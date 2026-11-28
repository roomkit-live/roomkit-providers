//! The client a provider is built on: the HTTP client, built once with the
//! times it gives a request, and what a provider states of its tries.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::header::AUTHORIZATION;
use roomkit_providers_http::TlsUnset;
use roomkit_providers_http::sse::STREAM_LIMIT;

use crate::endpoint::{has_user_information, percent_decoded};
use crate::exchange::Pace;
use crate::failure::{Exit, Failure, Secrets};
use crate::proxy::{Credentials, Proxies, Route};
use crate::retry::{Backoff, Rule};

/// For how long a request takes again a connection no request uses: the 5
/// seconds the client of Python's providers keeps one (httpx's
/// `keepalive_expiry`). The HTTP client closes those it no longer takes
/// when it looks at the connections it keeps, at this same interval: one
/// left with no request stays open between once and twice this time.
const IDLE_FOR: Duration = Duration::from_secs(5);

/// How many idle connections are kept for a host: the 100 the client of
/// Python's providers keeps (the SDKs' `max_keepalive_connections`).
const IDLE_AT_MOST: usize = 100;

/// The timeouts a client is built with: a configuration's.
///
/// [`new`](Self::new) is the only way to make them, so that a client is
/// built with none the clock cannot count: the two are read back
/// ([`read`](Self::read), [`connect`](Self::connect)) and never written.
///
/// ```compile_fail,E0451
/// use std::time::Duration;
///
/// let unchecked = roomkit_providers_client::Timeouts {
///     read: Duration::MAX,
///     connect: Duration::MAX,
/// };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeouts {
    read: Duration,
    connect: Duration,
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

    /// The longest wait for a read: for the head of an answer from the
    /// moment its request is sent, then between two reads of its body.
    #[must_use]
    pub const fn read(&self) -> Duration {
        self.read
    }

    /// The longest wait for a connection.
    #[must_use]
    pub const fn connect(&self) -> Duration {
        self.connect
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
/// - TLS is the configuration of `roomkit-providers-http`: ring and
///   HTTP/1.1, a server's certificate checked against the authorities
///   `SSL_CERT_FILE` or `SSL_CERT_DIR` name, else against the system's.
/// - A request goes through the proxy `HTTP_PROXY`, `HTTPS_PROXY` or
///   `ALL_PROXY` names for it, unless `NO_PROXY` exempts its host, as the
///   client of Python's providers reads those variables.
/// - The environment is read once, when the client is built: a variable
///   changed afterwards changes nothing for it.
/// - No redirect is followed, a proxy's own among them.
/// - No compressed answer is asked for, whatever compression a host's
///   build turns on in the HTTP client.
/// - A request takes again a connection no request used for 5 seconds or
///   less, as with the client of Python's providers, and the client closes
///   one left idle between 5 and 10 seconds after its last request. It
///   keeps 100 idle connections for a host at most.
/// - Every request names the client in its `User-Agent`
///   ([`USER_AGENT`](crate::USER_AGENT)), unless a header of the provider's
///   does.
#[derive(Clone)]
pub struct Client {
    pub(crate) http: reqwest::Client,
    timeouts: Timeouts,
    pub(crate) max_retries: u32,
    pub(crate) retry: Rule,
    /// The waits of an SDK that tries again only what got no answer, when
    /// the provider's does.
    pub(crate) unanswered_only: Option<Backoff>,
    pub(crate) refused: fn(u16, &[u8]) -> Failure,
    pub(crate) tried_again: fn(u32, Duration),
    pub(crate) pace: Pace,
    stream_limit: usize,
    /// The most bytes a whole answer is read to.
    pub(crate) answer_limit: usize,
    /// The most bytes an event of a streamed answer, and each of its lines,
    /// is read to.
    pub(crate) event_limit: usize,
    /// The proxies the environment named when the client was built, and
    /// whether the client still sends through them.
    route: Arc<Route>,
}

impl Client {
    /// The client of a provider that states `settings`. The environment is
    /// read here, and nowhere else: the proxy variables, and the variables
    /// that name the authorities a server's certificate is checked
    /// against.
    ///
    /// # Errors
    ///
    /// The failure of a provider that cannot be built: a proxy variable
    /// that does not read or names a proxy that is neither `http` nor
    /// `https`, a file `SSL_CERT_FILE` names that holds no certificate, a
    /// certificate store of the system that is there and does not read, or
    /// a TLS configuration or an HTTP client that could not be made.
    pub fn new(settings: Settings) -> Result<Self, Failure> {
        Self::with_proxies(settings, Proxies::of_the_environment()?)
    }

    /// A client for a managed local service. Requests bypass every proxy.
    ///
    /// # Errors
    /// The TLS configuration or HTTP client could not be built.
    pub fn direct(settings: Settings) -> Result<Self, Failure> {
        Self::with_proxies(settings, Proxies::default())
    }

    fn with_proxies(settings: Settings, proxies: Proxies) -> Result<Self, Failure> {
        let tls = roomkit_providers_http::tls_config();
        let tls = tls.map_err(|unset| without_tls(&unset))?;
        Self::built(settings, proxies, tls)
    }

    /// The client of a provider that states `settings`, which sends through
    /// `proxies` and checks a server's certificate as `tls` says: what
    /// [`new`](Self::new) builds of the environment, and what a test
    /// builds without one.
    pub(crate) fn built(
        settings: Settings,
        proxies: Proxies,
        tls: rustls::ClientConfig,
    ) -> Result<Self, Failure> {
        let route = Arc::new(Route::through(proxies));
        let timeouts = settings.timeouts;
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            // No compressed answer is asked for, whatever a host's build
            // turns on in the HTTP client.
            .no_gzip()
            .no_brotli()
            .no_zstd()
            .no_deflate()
            // The proxies are those read above, given below, and no other:
            // the HTTP client reads neither the environment nor the
            // system's settings itself.
            .no_proxy()
            .read_timeout(timeouts.read)
            .connect_timeout(timeouts.connect)
            // A connection no request uses is taken again for as long, and
            // as many of them are kept, as with the client of Python's
            // providers.
            .pool_idle_timeout(IDLE_FOR)
            .pool_max_idle_per_host(IDLE_AT_MOST)
            .redirect(reqwest::redirect::Policy::none());
        let rules = route.rules().into_iter();
        let http = rules.fold(http, reqwest::ClientBuilder::proxy);
        let http = http.build();
        let http = http.map_err(|_| Failure::unbuilt("the HTTP client could not be built"))?;
        Ok(Self {
            http,
            timeouts,
            max_retries: settings.max_retries,
            retry: settings.retry,
            unanswered_only: None,
            refused: settings.refused,
            tried_again: settings.tried_again,
            pace: Pace::default(),
            stream_limit: STREAM_LIMIT,
            answer_limit: crate::ANSWER_LIMIT,
            event_limit: roomkit_providers_http::sse::EVENT_LIMIT,
            route,
        })
    }

    /// Reads a whole answer to `answer` bytes at most, and an event of a
    /// streamed answer, and each of its lines, to `event` bytes at most: for
    /// a provider whose answers carry images, which the bounds of a model's
    /// answer do not hold (a generated image is megabytes of base64).
    pub fn read_up_to(&mut self, answer: usize, event: usize) {
        self.answer_limit = answer;
        self.event_limit = event;
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
    /// bound, which sends no answer of 256 MiB. A provider has no such
    /// call: it is there for the crate's tests and under the `harness`
    /// feature alone.
    #[cfg(any(test, feature = "harness"))]
    pub fn set_stream_limit(&mut self, limit: usize) {
        self.stream_limit = limit;
    }

    /// Tries a request again only when it got no answer, a lost connection
    /// or a timeout, after `backoff`'s waits, and never one a server
    /// answered, whatever its status: for a provider whose SDK tries again
    /// that way. Up to the settings' `max_retries`, as any try.
    pub fn try_again_only_unanswered(&mut self, backoff: Backoff) {
        self.unanswered_only = Some(backoff);
    }

    /// Replaces the clock a `retry-after` date is read against and the wait
    /// between two tries: for a test, which takes no wait. A provider has
    /// no such call: it is there for the crate's tests and under the
    /// `harness` feature alone.
    #[cfg(any(test, feature = "harness"))]
    pub fn set_pace(&mut self, pace: Pace) {
        self.pace = pace;
    }

    /// The clock and the waits the client takes: for a provider whose
    /// other requests, sent on a client of their own, keep to this one's in
    /// a test or a replay.
    #[cfg(any(test, feature = "replay", feature = "harness"))]
    #[must_use]
    pub fn pace(&self) -> Pace {
        self.pace.clone()
    }

    /// The seconds since 1970 by the client's clock: the system's, or the
    /// one a replay or a test set. What a provider dates from an answer it
    /// reads now, as a listing's prices, is dated by it.
    #[must_use]
    pub fn now(&self) -> f64 {
        (self.pace.now)()
    }

    /// Where the failures of the provider labelled `provider` leave: the
    /// one way a provider gets an [`Exit`], so that none lacks what the
    /// client alone knows, the credentials of the proxies its environment
    /// named.
    ///
    /// Its failures repeat nothing of the configuration's `keys`, of the
    /// value of each of its `headers` and of its credentials when it names
    /// a scheme before them, of the user information of `url`, the URL it
    /// sends to, nor of the user information of each proxy: as written and
    /// as it reads, and as the credentials the client writes of it, for
    /// the endpoint or for a proxy.
    #[must_use]
    pub fn exit<'h>(
        &self,
        provider: &'static str,
        keys: impl IntoIterator<Item = String>,
        headers: impl IntoIterator<Item = &'h str>,
        url: &reqwest::Url,
    ) -> Exit {
        Exit::new(provider, self.secrets(keys, headers, url))
    }

    /// What no failure may repeat of a configuration and of the process:
    /// what [`exit`](Self::exit) says.
    fn secrets<'h>(
        &self,
        keys: impl IntoIterator<Item = String>,
        headers: impl IntoIterator<Item = &'h str>,
        url: &reqwest::Url,
    ) -> Secrets {
        let mut texts: Vec<String> = keys.into_iter().collect();
        texts.extend(headers.into_iter().flat_map(with_credentials).flatten());
        texts.extend(self.user_information_of(url));
        let proxies = self.route.proxies().named();
        for given in proxies.filter_map(|(_, proxy)| proxy.credentials.as_ref()) {
            texts.extend(self.given_to_a_proxy(given));
        }
        Secrets::of(texts)
    }

    /// What a proxy is given, in each form a text may hold it in: its user
    /// information as the variable writes it and as the proxy reads it,
    /// each part and the two together, and the Basic credentials the
    /// client writes of it in a `Proxy-Authorization`.
    fn given_to_a_proxy(&self, given: &Credentials) -> Vec<String> {
        let (user, password) = (&given.user, &given.password);
        let written = given.written.split_once(':');
        let (written_user, written_password) = written.unwrap_or((&given.written, ""));
        let mut texts = vec![given.written.clone(), format!("{user}:{password}")];
        texts.extend([written_user, written_password, user, password].map(str::to_owned));
        // What the HTTP client writes of a user and a password: the same
        // for an endpoint as for a proxy.
        let written = self.http.get("http://proxy.invalid/");
        let written = written.basic_auth(user, Some(password)).build().ok();
        let written = written
            .as_ref()
            .and_then(|request| request.headers().get(AUTHORIZATION));
        let written = written.and_then(|value| value.to_str().ok());
        texts.extend(written.into_iter().flat_map(with_credentials).flatten());
        texts
    }

    /// The user information of `url` in each form a text may hold it in: as
    /// written and as it reads, each part and the two together, and the
    /// Basic credentials the client writes of it, in an `Authorization` to
    /// an endpoint as in a `Proxy-Authorization` to a proxy.
    fn user_information_of(&self, url: &reqwest::Url) -> Vec<String> {
        if !has_user_information(url) {
            return Vec::new();
        }
        let (user, password) = (url.username(), url.password().unwrap_or_default());
        let read = [percent_decoded(user), percent_decoded(password)];
        let mut texts = vec![user.to_owned(), password.to_owned()];
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
        texts
    }

    /// Sends each of `urls`, the URLs a provider asks, to `fake` instead, a
    /// local fake of its endpoint, over plain HTTP, each to the path it had
    /// and with the user information it had; and sends there directly,
    /// through no proxy the environment named: for a test of a provider,
    /// which a developer's proxy must neither change nor receive. A clone
    /// of the client goes directly too.
    ///
    /// `false`, with nothing changed, for a `fake` that is no loopback
    /// address; `false` too when a URL could not take the address.
    #[cfg(any(test, feature = "replay", feature = "harness"))]
    #[must_use]
    pub fn pointed_at<'u>(
        &mut self,
        urls: impl IntoIterator<Item = &'u mut reqwest::Url>,
        fake: std::net::SocketAddr,
    ) -> bool {
        if !fake.ip().is_loopback() {
            return false;
        }
        self.route.go_direct();
        let mut urls = urls.into_iter();
        urls.all(|url| crate::endpoint::moved_to(url, fake))
    }

    /// The client as a replay of recorded cases drives it, outside a
    /// provider crate's tests: a `retry-after` date is read against
    /// `clock`, in seconds since 1970, and no wait is taken between two
    /// tries: each is kept, in the waits returned. It sends directly from
    /// then on, through no proxy the environment named, and so do its
    /// clones: a replay sends nothing off the machine.
    #[cfg(feature = "replay")]
    pub fn replaying(&mut self, clock: f64) -> ReplayWaits {
        self.route.go_direct();
        let waits = ReplayWaits::default();
        let kept = waits.clone();
        self.pace = Pace {
            now: Arc::new(move || clock),
            wait: Arc::new(move |wait| {
                kept.keep(wait.as_secs_f64());
                Box::pin(std::future::ready(()))
            }),
        };
        waits
    }
}

impl fmt::Debug for Client {
    /// What the client was built with: nothing of a request, and of its
    /// proxies how many they are, never a URL.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("timeouts", &self.timeouts)
            .field("max_retries", &self.max_retries)
            .field("retry", &self.retry)
            .field("stream_limit", &self.stream_limit)
            .field("proxies", self.route.proxies())
            .finish_non_exhaustive()
    }
}

/// The failure of a provider whose TLS could not be set, which `unset` says
/// why: in its own words, which hold nothing of what a file contains.
fn without_tls(unset: &TlsUnset) -> Failure {
    let reason = unset.reason();
    Failure::before_request(format!("the provider cannot be built: {reason}"))
}

/// A header's value, and the credentials it holds when it names a scheme
/// before them (`Bearer`, `Basic`).
fn with_credentials(value: &str) -> [Option<String>; 2] {
    let after_a_scheme = value
        .split_once(' ')
        .map(|(_, rest)| rest.trim().to_owned());
    [Some(value.to_owned()), after_a_scheme]
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
pub struct ReplayWaits(Arc<std::sync::Mutex<Vec<f64>>>);

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
