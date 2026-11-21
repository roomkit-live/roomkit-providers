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
use roomkit_providers_core::ProviderError;

use crate::provider::{JSON, OpenAiAiProvider};
use crate::retry;

/// The most bytes of an answer read whole: a server that sends more fails
/// the request.
pub(crate) const ANSWER_LIMIT: usize = 4 * 1024 * 1024;

/// A wait the provider takes between two tries.
type Wait = Pin<Box<dyn Future<Output = ()> + Send>>;

/// The clock a `retry-after` date is read against, and the wait between
/// two tries: the system's, which a test replaces.
#[derive(Clone)]
pub(crate) struct Pace {
    /// The seconds since 1970.
    pub(crate) now: Arc<dyn Fn() -> f64 + Send + Sync>,
    /// Waits before the next try.
    pub(crate) wait: Arc<dyn Fn(Duration) -> Wait + Send + Sync>,
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

/// What is asked of the endpoint.
#[derive(Clone, Copy)]
pub(crate) enum Call<'b> {
    /// A completion: the body of its request.
    Chat(&'b [u8]),
    /// The models the endpoint lists.
    Models,
}

/// How a try ended without an answer.
enum Failed {
    /// The request failed for good.
    Final(ProviderError),
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

impl OpenAiAiProvider {
    /// Sends `call` until a whole answer comes or no try is left: the
    /// answer's body, and whether the server said it is JSON.
    pub(crate) async fn send(&self, call: Call<'_>) -> Result<(Vec<u8>, bool), ProviderError> {
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
    pub(crate) async fn open(&self, call: Call<'_>) -> Result<reqwest::Response, ProviderError> {
        let mut retries_taken = 0;
        loop {
            match self.head(call, retries_taken, false).await {
                Ok(response) => return Ok(response),
                Err(failed) => self.again(failed, &mut retries_taken).await?,
            }
        }
    }

    /// Waits before another try, or gives the failure that ends the request.
    async fn again(&self, failed: Failed, retries_taken: &mut u32) -> Result<(), ProviderError> {
        match failed {
            Failed::Final(error) => Err(error),
            Failed::Again(wait) => {
                tracing::info!(
                    target: "roomkit::providers::openai",
                    retries_taken = *retries_taken,
                    ?wait,
                    "a request is tried again"
                );
                (self.pace.wait)(wait).await;
                *retries_taken = retries_taken.saturating_add(1);
                Ok(())
            }
        }
    }

    /// Whether a try is left after `retries_taken`.
    fn left(&self, retries_taken: u32) -> bool {
        retries_taken < self.config.max_retries
    }

    /// The failure of a try that lost its connection: another try when one
    /// is left.
    fn lost(&self, error: &reqwest::Error, retries_taken: u32) -> Failed {
        if self.left(retries_taken) {
            let now = (self.pace.now)();
            Failed::Again(retry::delay(retries_taken, None, now))
        } else {
            Failed::Final(self.failing().transport(error))
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
                let message = format!("the provider's answer is longer than {ANSWER_LIMIT} bytes");
                Err(Failed::Final(self.failing().unreadable(&message)))
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
        let failing = self.failing();
        let mut headers = self.headers.clone();
        let request = match call {
            Call::Chat(body) => {
                // A configured header replaces the provider's own.
                let kind = headers.entry(CONTENT_TYPE);
                kind.or_insert(HeaderValue::from_static(JSON));
                self.client.post(self.chat.clone()).body(body.to_vec())
            }
            Call::Models => self.client.get(self.models.clone()),
        };
        let response = match request.headers(headers).send().await {
            Ok(response) => response,
            // A request that could not be built never left: it fails the
            // same however many times it is tried.
            Err(error) if error.is_builder() => {
                return Err(Failed::Final(failing.transport(&error)));
            }
            Err(error) => return Err(self.lost(&error, retries_taken)),
        };
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        if self.left(retries_taken) && retry::should_retry(status, response.headers(), now) {
            let wait = retry::delay(retries_taken, Some(response.headers()), now);
            return Err(Failed::Again(wait));
        }
        // An error body past what a request reads still keeps its status,
        // as Python's client reads the status whatever the body.
        let body = match read_body(response).await {
            Ok(body) => body,
            Err(Unread::TooLong) => Vec::new(),
            Err(Unread::Lost(error)) if whole => return Err(self.lost(&error, retries_taken)),
            Err(Unread::Lost(error)) => return Err(Failed::Final(failing.transport(&error))),
        };
        Err(Failed::Final(failing.status(status.as_u16(), &body)))
    }
}

/// Whether the server says its answer is JSON.
fn is_json(headers: &HeaderMap) -> bool {
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
