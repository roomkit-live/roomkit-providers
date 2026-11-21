//! One request and its tries: what Python's client does of a request that
//! fails, up to the configuration's `max_retries`.
//!
//! A whole answer is read inside its try, so an answer lost while it is
//! read is tried again. A streamed answer is tried until its head came:
//! what happens to it afterwards is the reader's.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};

use crate::client::Client;
use crate::failure::Failure;

/// The most bytes of an answer read whole: a server that sends more fails
/// the request.
pub const ANSWER_LIMIT: usize = 4 * 1024 * 1024;

/// What a request with a body says its body is, unless a header of the
/// provider's says otherwise.
const JSON: &str = "application/json";

/// What every request names the client with, in its `User-Agent`, unless a
/// header of the provider's does: `roomkit-providers/` and the version of
/// the workspace.
pub const USER_AGENT: &str = concat!("roomkit-providers/", env!("CARGO_PKG_VERSION"));

/// A wait a client takes between two tries.
pub type Wait = Pin<Box<dyn Future<Output = ()> + Send>>;

/// The clock a `retry-after` date is read against, and the wait between
/// two tries: the system's, which a test replaces
/// ([`Client::set_pace`]).
#[derive(Clone)]
pub struct Pace {
    /// The seconds since 1970.
    pub now: Arc<dyn Fn() -> f64 + Send + Sync>,
    /// Waits before the next try.
    pub wait: Arc<dyn Fn(Duration) -> Wait + Send + Sync>,
}

impl Default for Pace {
    fn default() -> Self {
        let now = || {
            let since = SystemTime::now().duration_since(UNIX_EPOCH);
            since.map_or(0.0, |since| since.as_secs_f64())
        };
        Self {
            now: Arc::new(now),
            wait: Arc::new(|wait| Box::pin(tokio::time::sleep(wait))),
        }
    }
}

impl std::fmt::Debug for Pace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pace").finish_non_exhaustive()
    }
}

/// What is asked of an endpoint: one request, as each of its tries sends
/// it.
#[derive(Clone, Copy, Debug)]
pub struct Call<'a> {
    /// Where the request is sent.
    pub url: &'a reqwest::Url,
    /// The headers the request carries. Every request also names the
    /// client that sends it ([`USER_AGENT`]), and one with a body says that
    /// its body is JSON, unless a header here already does.
    pub headers: &'a HeaderMap,
    /// The body of a `POST`, a JSON document; `None` for a `GET`.
    pub body: Option<&'a [u8]>,
}

/// How a try ended without an answer.
enum Failed {
    /// The request failed for good.
    Final(Failure),
    /// Another try comes after this wait.
    Again(Duration),
}

/// Why an answer's body was not read.
enum Unread {
    /// It is past [`ANSWER_LIMIT`].
    TooLong,
    /// The connection failed while it was read.
    Lost(reqwest::Error),
}

impl Client {
    /// Sends `call` until a whole answer comes or no try is left: the
    /// answer's body, within [`ANSWER_LIMIT`], and whether the server said
    /// it is JSON.
    ///
    /// # Errors
    ///
    /// The failure that ended the request: one that could not be built, a
    /// connection lost or an answer that did not come in time once no try
    /// is left, a redirect, a status the provider reads as a refusal, or an
    /// answer past [`ANSWER_LIMIT`].
    pub async fn send(&self, call: Call<'_>) -> Result<(Vec<u8>, bool), Failure> {
        let mut retries_taken = 0;
        loop {
            match self.whole(call, retries_taken).await {
                Ok(answered) => return Ok(answered),
                Err(failed) => self.again(failed, &mut retries_taken).await?,
            }
        }
    }

    /// Sends `call` until the head of an answer comes or no try is left:
    /// the answer, its body still to read.
    pub(crate) async fn open(&self, call: Call<'_>) -> Result<reqwest::Response, Failure> {
        let mut retries_taken = 0;
        loop {
            match self.head(call, retries_taken, false).await {
                Ok(response) => return Ok(response),
                Err(failed) => self.again(failed, &mut retries_taken).await?,
            }
        }
    }

    /// Waits before another try, or gives the failure that ends the request.
    async fn again(&self, failed: Failed, retries_taken: &mut u32) -> Result<(), Failure> {
        match failed {
            Failed::Final(error) => Err(error),
            Failed::Again(wait) => {
                (self.tried_again)(*retries_taken, wait);
                (self.pace.wait)(wait).await;
                *retries_taken = retries_taken.saturating_add(1);
                Ok(())
            }
        }
    }

    /// Whether a try is left after `retries_taken`.
    fn left(&self, retries_taken: u32) -> bool {
        retries_taken < self.max_retries
    }

    /// The failure of a try that lost its connection: another try when one
    /// is left.
    fn lost(&self, error: &reqwest::Error, retries_taken: u32) -> Failed {
        if self.left(retries_taken) {
            let now = (self.pace.now)();
            Failed::Again(self.retry.delay(retries_taken, None, now))
        } else {
            Failed::Final(Failure::transport(error))
        }
    }

    /// One try, its answer read whole.
    async fn whole(&self, call: Call<'_>, retries_taken: u32) -> Result<(Vec<u8>, bool), Failed> {
        let response = self.head(call, retries_taken, true).await?;
        let json = is_json(response.headers());
        match read_body(response).await {
            Ok(body) => Ok((body, json)),
            Err(Unread::Lost(error)) => Err(self.lost(&error, retries_taken)),
            Err(Unread::TooLong) => {
                let said = format!("the provider's answer is longer than {ANSWER_LIMIT} bytes");
                Err(Failed::Final(Failure::unreadable(said)))
            }
        }
    }

    /// One try up to the head of its answer: the answer when its status is
    /// a success, the failure that ends the request, or the wait before
    /// another try. The body of a refusal is read here; one lost while it
    /// is read is tried again only for an answer asked `whole`, as Python's
    /// client reads a refused stream outside its tries.
    async fn head(
        &self,
        call: Call<'_>,
        retries_taken: u32,
        whole: bool,
    ) -> Result<reqwest::Response, Failed> {
        let now = (self.pace.now)();
        let mut headers = call.headers.clone();
        // A header of the provider's replaces the client's own. The name is
        // written on the request, not given to the HTTP client, which would
        // also send it to a proxy that opens a tunnel.
        let agent = headers.entry(reqwest::header::USER_AGENT);
        agent.or_insert(HeaderValue::from_static(USER_AGENT));
        let request = match call.body {
            Some(body) => {
                let kind = headers.entry(CONTENT_TYPE);
                kind.or_insert(HeaderValue::from_static(JSON));
                self.http.post(call.url.clone()).body(body.to_vec())
            }
            None => self.http.get(call.url.clone()),
        };
        let response = match request.headers(headers).send().await {
            Ok(response) => response,
            // A request that could not be built never left: it fails the
            // same however many times it is tried, so it is final.
            Err(error) if error.is_builder() => {
                let unbuilt = Failure::before_request("the request could not be built");
                return Err(Failed::Final(unbuilt));
            }
            Err(error) => return Err(self.lost(&error, retries_taken)),
        };
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let answered = response.headers();
        if self.left(retries_taken) && self.retry.should_retry(status, answered, now) {
            let wait = self.retry.delay(retries_taken, Some(answered), now);
            return Err(Failed::Again(wait));
        }
        // An error body past what a request reads still keeps its status,
        // as Python's client reads the status whatever the body.
        let body = match read_body(response).await {
            Ok(body) => body,
            Err(Unread::TooLong) => Vec::new(),
            Err(Unread::Lost(error)) if whole => return Err(self.lost(&error, retries_taken)),
            Err(Unread::Lost(error)) => return Err(Failed::Final(Failure::transport(&error))),
        };
        // A redirect is a failure: it is not followed.
        let failure = if status.is_redirection() {
            Failure::redirect(status.as_u16())
        } else {
            (self.refused)(status.as_u16(), &body)
        };
        Err(Failed::Final(failure))
    }
}

/// Whether the server says its answer is JSON.
#[must_use]
pub fn is_json(headers: &HeaderMap) -> bool {
    let kind = headers.get(CONTENT_TYPE);
    let kind = kind.and_then(|value| value.to_str().ok());
    kind.is_some_and(|value| value.trim_start().starts_with("application/json"))
}

/// The body of an answer, within [`ANSWER_LIMIT`].
async fn read_body(mut response: reqwest::Response) -> Result<Vec<u8>, Unread> {
    let declared = response.content_length().unwrap_or_default();
    if usize::try_from(declared).map_or(true, |declared| declared > ANSWER_LIMIT) {
        return Err(Unread::TooLong);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(Unread::Lost)? {
        if body.len().saturating_add(chunk.len()) > ANSWER_LIMIT {
            return Err(Unread::TooLong);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests;
