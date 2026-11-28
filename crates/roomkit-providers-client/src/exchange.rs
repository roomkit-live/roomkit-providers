//! One request and its tries: what Python's client does of a request that
//! fails, up to the configuration's `max_retries`.
//!
//! A whole answer is read inside its try, so an answer lost while it is
//! read is tried again. A streamed answer is tried until its head came:
//! what happens to it afterwards is the reader's.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};

use crate::client::Client;
use crate::credential::TokenSource;
use crate::endpoint::{has_user_information, shown_url};
use crate::failure::{Failure, Secrets};

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
/// two tries: the system's, which a test replaces (`Client::set_pace`,
/// under the `harness` feature).
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

impl fmt::Debug for Pace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pace").finish_non_exhaustive()
    }
}

/// What is asked of an endpoint: one request, as each of its tries sends
/// it.
#[derive(Clone, Copy)]
pub struct Call<'a> {
    /// Where the request is sent. Its user information, when it holds
    /// some, is sent as Basic credentials.
    pub url: &'a reqwest::Url,
    /// The headers the request carries. Every request also names the
    /// client that sends it ([`USER_AGENT`]), and one with a body says that
    /// its body is JSON, unless a header here already does. An
    /// `Authorization` here is not sent where the URL holds user
    /// information: the Basic credentials are the request's, as with
    /// Python's client.
    pub headers: &'a HeaderMap,
    /// The body of a `POST`, a JSON document; `None` for a `GET`.
    pub body: Option<&'a [u8]>,
    /// The host's source of the bearer token the request carries, asked
    /// before each of its tries: the token is the try's `Authorization`,
    /// in place of one among `headers`, and nothing the answer says shows
    /// it. `None` for a request that carries what its headers say.
    pub token: Option<&'a dyn TokenSource>,
}

impl fmt::Debug for Call<'_> {
    /// Where the request goes, by its origin, the names of its headers and
    /// how many bytes its body is: never the user information, the path or
    /// the query of its URL, the value of a header or the body, each of
    /// which may hold a secret.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Vec<&str> = self.headers.keys().map(HeaderName::as_str).collect();
        f.debug_struct("Call")
            .field("url", &shown_url(self.url.as_str()))
            .field("headers", &headers)
            .field("body_bytes", &self.body.map(<[u8]>::len))
            .field("token", &self.token.is_some())
            .finish()
    }
}

/// A header of a provider's configuration, as a request carries it: its
/// name, and its value, which no `Debug` form shows, since a host may put a
/// credential in a header of any name. `None` for a name or a value that
/// is no HTTP header's.
#[must_use]
pub fn configured_header(name: &str, value: &str) -> Option<(HeaderName, HeaderValue)> {
    let name = HeaderName::from_bytes(name.as_bytes()).ok()?;
    let mut value = HeaderValue::from_str(value).ok()?;
    value.set_sensitive(true);
    Some((name, value))
}

/// What an answer is shown without: the token its try was sent with, which
/// a server may send back. It is removed from what the client hands on of
/// the answer, so that no failure built from it, whoever builds it, holds
/// the token. An answer that is no UTF-8 text is handed on as it came.
#[derive(Clone, Default)]
pub(crate) struct Hidden(Option<Secrets>);

impl Hidden {
    fn of(token: Option<&str>) -> Self {
        Self(token.map(|token| Secrets::of([token.to_owned()])))
    }

    /// `text` without the token.
    pub(crate) fn text(&self, text: String) -> String {
        match &self.0 {
            Some(token) => token.out_of(&text),
            None => text,
        }
    }

    /// `body` without the token.
    pub(crate) fn bytes(&self, body: Vec<u8>) -> Vec<u8> {
        match (&self.0, std::str::from_utf8(&body)) {
            (Some(token), Ok(text)) => token.out_of(text).into_bytes(),
            _ => body,
        }
    }
}

/// The head of an answer, and what the answer is shown without.
pub(crate) struct Opened {
    pub(crate) response: reqwest::Response,
    pub(crate) hidden: Hidden,
}

/// The `Authorization` of a try whose call names a source of tokens: the
/// token is the try's own, in place of one among the call's headers.
enum Bearer {
    /// The token, as a bearer.
    Token(HeaderValue),
    /// The source gave an empty token: the try carries no `Authorization`,
    /// and never the one among the call's headers in its place.
    Absent,
}

/// The `Authorization` of a try that carries `token`.
fn bearer(token: &str) -> Result<Bearer, Failed> {
    if token.is_empty() {
        return Ok(Bearer::Absent);
    }
    let no_header = || {
        let said = "the token of the host's source is no value of a header";
        Failed::Final(Failure::before_request(said))
    };
    let mut bearer = HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| no_header())?;
    bearer.set_sensitive(true);
    Ok(Bearer::Token(bearer))
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
        let (body, json, _) = self.send_headed(call).await?;
        Ok((body, json))
    }

    /// As [`send`](Self::send), with the headers of the answer: what a
    /// provider reads of it beside its body, the vendor's id of the request
    /// (`x-request-id`).
    ///
    /// # Errors
    ///
    /// As [`send`](Self::send).
    pub async fn send_headed(&self, call: Call<'_>) -> Result<(Vec<u8>, bool, HeaderMap), Failure> {
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
    pub(crate) async fn open(&self, call: Call<'_>) -> Result<Opened, Failure> {
        let mut retries_taken = 0;
        loop {
            match self.head(call, retries_taken, false).await {
                Ok(opened) => return Ok(opened),
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
            let wait = match self.unanswered_only {
                Some(backoff) => backoff.delay(retries_taken),
                None => self.retry.delay(retries_taken, None, now),
            };
            Failed::Again(wait)
        } else {
            Failed::Final(Failure::transport(error))
        }
    }

    /// The request of one try of `call`, with what the client writes of
    /// its own beside the call's headers, and `bearer`, the token of the
    /// host's source for this try, as its `Authorization`.
    fn request(&self, call: Call<'_>, bearer: Option<Bearer>) -> reqwest::RequestBuilder {
        let mut headers = call.headers.clone();
        match bearer {
            Some(Bearer::Token(bearer)) => drop(headers.insert(AUTHORIZATION, bearer)),
            Some(Bearer::Absent) => drop(headers.remove(AUTHORIZATION)),
            None => {}
        }
        // A header of the provider's replaces the client's own. The name is
        // written on the request, not given to the HTTP client, which would
        // also send it to a proxy that opens a tunnel.
        let agent = headers.entry(reqwest::header::USER_AGENT);
        agent.or_insert(HeaderValue::from_static(USER_AGENT));
        // The user information of a URL goes as Basic credentials, which
        // the HTTP client writes: they are the request's `Authorization`,
        // in place of one among the call's headers, as with Python's
        // client.
        if has_user_information(call.url) {
            headers.remove(AUTHORIZATION);
        }
        let request = match call.body {
            Some(body) => {
                let kind = headers.entry(CONTENT_TYPE);
                kind.or_insert(HeaderValue::from_static(JSON));
                self.http.post(call.url.clone()).body(body.to_vec())
            }
            None => self.http.get(call.url.clone()),
        };
        request.headers(headers)
    }

    /// One try, its answer read whole.
    async fn whole(
        &self,
        call: Call<'_>,
        retries_taken: u32,
    ) -> Result<(Vec<u8>, bool, HeaderMap), Failed> {
        let Opened { response, hidden } = self.head(call, retries_taken, true).await?;
        let json = is_json(response.headers());
        let headers = response.headers().clone();
        match read_body(response, self.answer_limit).await {
            Ok(body) => Ok((hidden.bytes(body), json, headers)),
            Err(Unread::Lost(error)) => Err(self.lost(&error, retries_taken)),
            Err(Unread::TooLong) => {
                let limit = self.answer_limit;
                let said = format!("the provider's answer is longer than {limit} bytes");
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
    ) -> Result<Opened, Failed> {
        let now = (self.pace.now)();
        // The host's source is asked before each try: a token it could not
        // give ends the request, with no try left to take.
        let token = match call.token {
            Some(source) => {
                let asked = source.token().await;
                Some(asked.map_err(|error| Failed::Final(Failure::credential(error)))?)
            }
            None => None,
        };
        let hidden = Hidden::of(token.as_deref());
        let bearer = token.as_deref().map(bearer).transpose()?;
        let response = match self.request(call, bearer).send().await {
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
            return Ok(Opened { response, hidden });
        }
        let answered = response.headers();
        let answers_retried = self.unanswered_only.is_none();
        if answers_retried
            && self.left(retries_taken)
            && self.retry.should_retry(status, answered, now)
        {
            let wait = self.retry.delay(retries_taken, Some(answered), now);
            return Err(Failed::Again(wait));
        }
        // An error body past what a request reads still keeps its status,
        // as Python's client reads the status whatever the body.
        let body = match read_body(response, self.answer_limit).await {
            Ok(body) => body,
            Err(Unread::TooLong) => Vec::new(),
            Err(Unread::Lost(error)) if whole => return Err(self.lost(&error, retries_taken)),
            Err(Unread::Lost(error)) => return Err(Failed::Final(Failure::transport(&error))),
        };
        // A redirect is a failure: it is not followed.
        let failure = if status.is_redirection() {
            Failure::redirect(status.as_u16())
        } else {
            (self.refused)(status.as_u16(), &hidden.bytes(body))
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

/// The body of an answer, within `limit` bytes: [`ANSWER_LIMIT`] unless
/// the provider reads larger answers.
async fn read_body(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, Unread> {
    let declared = response.content_length().unwrap_or_default();
    if usize::try_from(declared).map_or(true, |declared| declared > limit) {
        return Err(Unread::TooLong);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(Unread::Lost)? {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(Unread::TooLong);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests;
