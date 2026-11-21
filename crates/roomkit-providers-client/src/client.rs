//! The client a provider is built on: the HTTP client, built once with the
//! times it gives a request, and what a provider states of its tries.

use std::fmt;
use std::time::{Duration, Instant};

use reqwest::header::AUTHORIZATION;
use roomkit_providers_http::sse::STREAM_LIMIT;

use crate::endpoint::{has_user_information, percent_decoded};
use crate::exchange::Pace;
use crate::failure::{Failure, Secrets};
use crate::retry::Rule;

/// How long a connection no request uses is kept: the 5 seconds the client
/// of Python's providers keeps one (httpx's `keepalive_expiry`).
const IDLE_FOR: Duration = Duration::from_secs(5);

/// How many idle connections are kept for a host: the 100 the client of
/// Python's providers keeps (the SDKs' `max_keepalive_connections`).
const IDLE_AT_MOST: usize = 100;

/// The timeouts a client is built with: a configuration's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeouts {
    /// The longest wait for a read: for the head of an answer from the
    /// moment its request is sent, then between two reads of its body.
    pub read: Duration,
    /// The longest wait for a connection.
    pub connect: Duration,
}

/// How long past its timeout a clock must still count: a provider outlives
/// no such time, so a timeout the clock can add once it can add always.
const HEADROOM: Duration = Duration::from_hours(100 * 365 * 24);

impl Timeouts {
    /// The timeouts of a configuration: `read` for the head of an answer
    /// and between two reads of its body, `connect` to connect.
    ///
    /// # Errors
    ///
    /// A timeout so long that the clock cannot count to its end: the HTTP
    /// client would fail on it at the first read.
    pub fn new(read: Duration, connect: Duration) -> Result<Self, Failure> {
        let counted = |timeout: Duration| {
            let end = Instant::now().checked_add(timeout);
            end.and_then(|end| end.checked_add(HEADROOM)).is_some()
        };
        if !counted(read) || !counted(connect) {
            return Err(Failure::unbuilt(
                "its timeout is longer than the clock can count",
            ));
        }
        Ok(Self { read, connect })
    }
}

/// What a provider states of the client it is built on.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// The timeouts of the provider's configuration.
    pub timeouts: Timeouts,
    /// How many times a request that fails is tried again: the
    /// configuration's `max_retries`.
    pub max_retries: u32,
    /// Where the retry rule of the provider's SDK differs from another's.
    pub retry: Rule,
    /// How the provider reads the answer of a request its server refused,
    /// a status that is neither a success nor a redirect, with its body:
    /// the failure it words of it, [`Failure::refused`] of what the body
    /// states, or one in words of its own.
    pub refused: fn(status: u16, body: &[u8]) -> Failure,
    /// Says that a request is tried again, after how many tries and what
    /// wait: [`tried_again!`](crate::tried_again), which logs it under the
    /// provider's own target.
    pub tried_again: fn(retries_taken: u32, wait: Duration),
}

/// The HTTP client a provider is built on. It sends a request until an
/// answer comes or no try is left ([`send`](Self::send),
/// [`stream`](Self::stream)), and bounds what it reads of each answer.
///
/// - TLS is the configuration of `roomkit-providers-http`: ring, the public
///   web PKI roots, HTTP/1.1.
/// - No redirect is followed, and no proxy is read from the system or its
///   environment.
/// - No compressed answer is asked for, whatever compression a host's
///   build turns on in the HTTP client.
/// - A connection no request uses is kept 5 seconds, 100 of them for a
///   host, as the client of Python's providers keeps them.
#[derive(Clone)]
pub struct Client {
    pub(crate) http: reqwest::Client,
    timeouts: Timeouts,
    pub(crate) max_retries: u32,
    pub(crate) retry: Rule,
    pub(crate) refused: fn(u16, &[u8]) -> Failure,
    pub(crate) tried_again: fn(u32, Duration),
    pub(crate) pace: Pace,
    stream_limit: usize,
}

impl Client {
    /// The client of a provider that states `settings`.
    ///
    /// # Errors
    ///
    /// The failure of a provider that cannot be built: the TLS
    /// configuration or the HTTP client could not be made.
    pub fn new(settings: Settings) -> Result<Self, Failure> {
        let tls = roomkit_providers_http::tls_config();
        let tls = tls.map_err(|_| Failure::unbuilt("TLS is not set"))?;
        let timeouts = settings.timeouts;
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            // No compressed answer is asked for, whatever a host's build
            // turns on in the HTTP client.
            .no_gzip()
            .no_brotli()
            .no_zstd()
            .no_deflate()
            // Neither the system's proxies nor its variables are read.
            .no_proxy()
            .read_timeout(timeouts.read)
            .connect_timeout(timeouts.connect)
            // A connection no request uses is kept as long, and as many of
            // them, as the client of Python's providers keeps.
            .pool_idle_timeout(IDLE_FOR)
            .pool_max_idle_per_host(IDLE_AT_MOST)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| Failure::unbuilt("the HTTP client could not be built"))?;
        Ok(Self {
            http,
            timeouts,
            max_retries: settings.max_retries,
            retry: settings.retry,
            refused: settings.refused,
            tried_again: settings.tried_again,
            pace: Pace::default(),
            stream_limit: STREAM_LIMIT,
        })
    }

    /// The timeouts the client was built with.
    #[must_use]
    pub fn timeouts(&self) -> Timeouts {
        self.timeouts
    }

    /// The most bytes read of a streamed answer: the bound of
    /// `roomkit-providers-http`'s reader, unless a test lowered it.
    #[must_use]
    pub fn stream_limit(&self) -> usize {
        self.stream_limit
    }

    /// Reads `limit` bytes at most of a streamed answer: for a test of the
    /// bound, which sends no answer of 256 MiB.
    pub fn set_stream_limit(&mut self, limit: usize) {
        self.stream_limit = limit;
    }

    /// Replaces the clock a `retry-after` date is read against and the wait
    /// between two tries: for a test, or a replay, which takes no wait.
    pub fn set_pace(&mut self, pace: Pace) {
        self.pace = pace;
    }

    /// What no failure may repeat of a configuration: its `keys`, the value
    /// of each of its `headers` and its credentials when it names a scheme
    /// before them, the user information of `url`, the URL it sends to, as
    /// written and as it reads, and the `Authorization` the client writes
    /// from that user information.
    pub fn secrets<'h>(
        &self,
        keys: impl IntoIterator<Item = String>,
        headers: impl IntoIterator<Item = &'h str>,
        url: &reqwest::Url,
    ) -> Secrets {
        let mut texts: Vec<String> = keys.into_iter().collect();
        let with_credentials = |value: &str| {
            let after_a_scheme = value
                .split_once(' ')
                .map(|(_, rest)| rest.trim().to_owned());
            [Some(value.to_owned()), after_a_scheme]
        };
        texts.extend(headers.into_iter().flat_map(with_credentials).flatten());
        if has_user_information(url) {
            let (user, password) = (url.username(), url.password().unwrap_or_default());
            let read = [percent_decoded(user), percent_decoded(password)];
            texts.extend([user.to_owned(), password.to_owned()]);
            texts.push(format!("{user}:{password}"));
            texts.push(read.join(":"));
            texts.extend(read);
            // What the client itself writes of it on each request.
            let written = self.http.get(url.clone()).build().ok();
            let written = written
                .as_ref()
                .and_then(|request| request.headers().get(AUTHORIZATION));
            let written = written.and_then(|value| value.to_str().ok());
            texts.extend(written.into_iter().flat_map(with_credentials).flatten());
        }
        Secrets::of(texts)
    }

    /// The client as a replay of recorded cases drives it, outside a
    /// provider crate's tests: a `retry-after` date is read against
    /// `clock`, in seconds since 1970, and no wait is taken between two
    /// tries: each is kept, in the waits returned.
    #[cfg(feature = "replay")]
    pub fn replaying(&mut self, clock: f64) -> ReplayWaits {
        let waits = ReplayWaits::default();
        let kept = waits.clone();
        self.pace = Pace {
            now: std::sync::Arc::new(move || clock),
            wait: std::sync::Arc::new(move |wait| {
                kept.keep(wait.as_secs_f64());
                Box::pin(std::future::ready(()))
            }),
        };
        waits
    }
}

impl fmt::Debug for Client {
    /// What the client was built with: nothing of a request.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("timeouts", &self.timeouts)
            .field("max_retries", &self.max_retries)
            .field("retry", &self.retry)
            .field("stream_limit", &self.stream_limit)
            .finish_non_exhaustive()
    }
}

/// Sends each of `urls`, the URLs a provider asks, to `fake` instead, a
/// local fake of its endpoint, over plain HTTP, each to the path it had and
/// with the user information it had: for a replay of recorded cases outside
/// a provider crate's tests.
///
/// # Errors
///
/// A `fake` that is no loopback address: a replay sends nothing off the
/// machine.
#[cfg(feature = "replay")]
pub fn replayed_at<'u>(
    urls: impl IntoIterator<Item = &'u mut reqwest::Url>,
    fake: std::net::SocketAddr,
) -> Result<(), Failure> {
    let no_loopback = || Failure::unbuilt("a replay sends to a loopback address only");
    if !fake.ip().is_loopback() {
        return Err(no_loopback());
    }
    let mut urls = urls.into_iter();
    if urls.all(|url| crate::endpoint::moved_to(url, fake)) {
        Ok(())
    } else {
        Err(no_loopback())
    }
}

/// The waits a replayed client did not take between its tries (see
/// [`Client::replaying`]).
#[cfg(feature = "replay")]
#[derive(Clone, Debug, Default)]
pub struct ReplayWaits(std::sync::Arc<std::sync::Mutex<Vec<f64>>>);

#[cfg(feature = "replay")]
impl ReplayWaits {
    fn keep(&self, seconds: f64) {
        let mut waits = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        waits.push(seconds);
    }

    /// The waits so far, in seconds, in the order they were asked.
    #[must_use]
    pub fn taken(&self) -> Vec<f64> {
        let waits = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        waits.clone()
    }
}

/// How a provider says that a request is tried again, for
/// [`Settings::tried_again`]: one line at the `info` level under `$target`,
/// the provider's own log target, with how many tries were taken and the
/// wait before the next.
///
/// ```
/// use std::time::Duration;
///
/// let tried_again: fn(u32, Duration) =
///     roomkit_providers_client::tried_again!("roomkit::providers::vendor");
/// tried_again(0, Duration::from_millis(500));
/// ```
#[macro_export]
macro_rules! tried_again {
    ($target:literal) => {
        |retries_taken: u32, wait: ::std::time::Duration| {
            $crate::__tracing::info!(
                target: $target,
                retries_taken,
                ?wait,
                "a request is tried again"
            );
        }
    };
}

#[cfg(test)]
mod tests;
