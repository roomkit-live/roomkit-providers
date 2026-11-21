//! What a failed request reads as, on every provider: Python's
//! `provider_error` and its helpers.
//!
//! A failure is built as a [`Failure`], which keeps the provider's own
//! words apart from what a server, a reader or the contract said. It leaves
//! the provider through [`Exit::leave`], the one place the contract's error
//! is made: there what was said is cleaned of the configuration's secrets,
//! put on one line and bounded.
//!
//! The failures every provider words alike are built here: a request that
//! never left, a lost connection, a redirect, a body that does not read, an
//! error object in place of an answer. A provider words its own with
//! [`Failure::said`], from the pieces [`own`] and [`told`] make, and reads
//! the body of a refused request into a [`Stated`], which
//! [`Failure::refused`] words.

use std::borrow::Cow;
use std::fmt;

use roomkit_providers_http::SseError;
use roomkit_providers_llm::{ProviderError, is_context_overflow_message};
use serde_json::Value;

pub use self::exit::{Exit, MARK, SECRET_MIN, Secrets, carried, one_line};
pub use self::words::names_transient;

mod exit;
mod words;

/// The most characters of a failure's text, and of what it repeats of a
/// server's answer.
pub const MESSAGE_LIMIT: usize = 500;

/// The most characters of an error's type or code, or of a value a reader
/// refused, a failure's text carries.
pub const NAME_LIMIT: usize = 64;

/// The status an error object's type names, where a server wrote the
/// failure into its body rather than its status line (Python's
/// `_ERROR_TYPE_STATUS`).
const ERROR_TYPE_STATUS: [(&str, u16); 21] = [
    ("invalid_request_error", 400),
    ("authentication_error", 401),
    ("permission_error", 403),
    ("not_found_error", 404),
    ("request_too_large", 413),
    ("rate_limit_error", 429),
    ("rate_limit_exceeded", 429),
    ("api_error", 500),
    ("server_error", 500),
    ("internal_error", 500),
    ("service_unavailable", 503),
    ("timeout_error", 504),
    ("overloaded_error", 529),
    ("INVALID_ARGUMENT", 400),
    ("UNAUTHENTICATED", 401),
    ("PERMISSION_DENIED", 403),
    ("NOT_FOUND", 404),
    ("RESOURCE_EXHAUSTED", 429),
    ("INTERNAL", 500),
    ("UNAVAILABLE", 503),
    ("DEADLINE_EXCEEDED", 504),
];

/// A piece of a failure's text: the provider's own words ([`own`]), or what
/// a server or a reader said ([`told`]).
#[derive(Clone)]
pub struct Said(Piece);

#[derive(Clone)]
enum Piece {
    /// The provider's own words: never searched for a secret.
    Own(Cow<'static, str>),
    /// What a server or a reader said, and the most characters of it the
    /// text carries: cleaned when the failure leaves.
    Told(String, usize),
}

impl fmt::Debug for Said {
    /// Whose words it holds: never the words, which are not cleaned yet.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whose = match self.0 {
            Piece::Own(_) => "own",
            Piece::Told(..) => "told",
        };
        f.debug_tuple("Said").field(&whose).finish()
    }
}

/// The provider's own words, which a failure carries as they are.
pub fn own(text: impl Into<Cow<'static, str>>) -> Said {
    Said(Piece::Own(text.into()))
}

/// What a server or a reader said, of which a failure carries `limit`
/// characters at most, cleaned of the configuration's secrets and put on
/// one line when it leaves.
pub fn told(text: impl Into<String>, limit: usize) -> Said {
    Said(Piece::Told(text.into(), limit))
}

/// How a failure was built.
#[derive(Clone)]
enum Built {
    /// By a provider, its text in pieces.
    Here {
        said: Vec<Said>,
        retryable: bool,
        status: Option<u16>,
        context_overflow: Option<bool>,
        before_request: bool,
    },
    /// By the contract: its text is one piece.
    Elsewhere(ProviderError),
}

/// A failure inside a provider. It becomes the contract's error only where
/// it leaves ([`Exit::leave`]).
#[derive(Clone)]
pub struct Failure(Built);

impl fmt::Debug for Failure {
    /// Where it was built: never what it says, which is not cleaned yet.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let built = match self.0 {
            Built::Here { .. } => "here",
            Built::Elsewhere(_) => "elsewhere",
        };
        f.debug_tuple("Failure").field(&built).finish()
    }
}

impl From<ProviderError> for Failure {
    /// A failure the contract built: of a request it refuses, or of an
    /// answer it checked.
    fn from(error: ProviderError) -> Self {
        Self(Built::Elsewhere(error))
    }
}

/// What the body of a refused request states of its failure, as its
/// provider reads it: what [`Failure::refused`] words.
#[derive(Clone, Default)]
pub struct Stated {
    /// The error's type.
    pub kind: Option<String>,
    /// The error's code.
    pub code: Option<String>,
    /// The error's message: `None` leaves the status's reason in its place.
    pub message: Option<String>,
    /// Whether the body names an overflow of the model's context otherwise
    /// than in words, by a code of the provider's.
    pub overflow: bool,
}

impl fmt::Debug for Stated {
    /// Which members the body states: never what they say, which is not
    /// cleaned yet.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stated")
            .field("kind", &self.kind.is_some())
            .field("code", &self.code.is_some())
            .field("message", &self.message.is_some())
            .field("overflow", &self.overflow)
            .finish()
    }
}

impl Failure {
    /// A failure in a provider's own wording: the pieces of its text,
    /// whether a new request may succeed, and the status it carries.
    #[must_use]
    pub fn said(said: Vec<Said>, retryable: bool, status: Option<u16>) -> Self {
        Self(Built::Here {
            said,
            retryable,
            status,
            context_overflow: None,
            before_request: false,
        })
    }

    /// The failure, saying that the request overflowed the model's context.
    /// One the contract built is left as it is.
    #[must_use]
    pub fn overflowing(mut self) -> Self {
        if let Built::Here {
            context_overflow, ..
        } = &mut self.0
        {
            *context_overflow = Some(true);
        }
        self
    }

    /// A failure before any request, final: `said`, the provider's own
    /// words.
    pub fn before_request(said: impl Into<Cow<'static, str>>) -> Self {
        Self(Built::Here {
            said: vec![own(said)],
            retryable: false,
            status: None,
            context_overflow: None,
            before_request: true,
        })
    }

    /// The failure of a request that could not be written as JSON: final,
    /// and nothing was sent.
    #[must_use]
    pub fn unwritten() -> Self {
        Self::before_request("the request could not be written as JSON")
    }

    /// The failure of a provider that cannot be built: before any request,
    /// final.
    #[must_use]
    pub fn unbuilt(reason: &'static str) -> Self {
        Self::before_request(format!("the provider cannot be built: {reason}"))
    }

    /// The failure of an answer whose status is no success, retried when
    /// the status is transient: `the provider answered HTTP 401 (type,
    /// code): message`. Of the `body` it carries what `stated` holds, what
    /// the provider read of an error object in it, and nothing else: where
    /// no message is stated, the text is the status's reason. Whether the
    /// request overflowed the model's context is read from the whole body,
    /// echoed or not: the code `stated` says names one, or the words.
    #[must_use]
    pub fn refused(status: u16, body: &[u8], stated: Stated) -> Self {
        let mut said = vec![own(format!("the provider answered HTTP {status}"))];
        let name = |name: String| told(name, NAME_LIMIT);
        match (stated.kind, stated.code) {
            (Some(kind), Some(code)) => {
                said.extend([own(" ("), name(kind), own(", "), name(code), own(")")]);
            }
            (Some(named), None) | (None, Some(named)) => {
                said.extend([own(" ("), name(named), own(")")]);
            }
            (None, None) => {}
        }
        said.push(own(": "));
        let reason = || {
            let status = reqwest::StatusCode::from_u16(status).ok()?;
            status.canonical_reason()
        };
        said.push(match stated.message {
            Some(message) => told(message, MESSAGE_LIMIT),
            None => own(reason().unwrap_or("no message")),
        });
        let failure = Self::said(said, retryable_status(status), Some(status));
        // Everything the server said is read for an overflow, not only
        // what is echoed.
        let by_words = is_context_overflow_message(&String::from_utf8_lossy(body));
        if stated.overflow || by_words {
            failure.overflowing()
        } else {
            failure
        }
    }

    /// The failure of an answer that is a redirect, final: a redirect is
    /// not followed.
    #[must_use]
    pub fn redirect(status: u16) -> Self {
        let said =
            format!("the endpoint answered with a redirect ({status}), which is not followed");
        Self::said(vec![own(said)], false, Some(status))
    }

    /// The failure of an error a server wrote inside its stream, in place
    /// of an event: the status the error names decides whether it is
    /// retried, else its words (Python's `provider_error`). Its text is
    /// the error's own message, or `unworded`, the words of Python's client
    /// for an error that brings none.
    #[must_use]
    pub fn stream_error(error: &Value, unworded: &'static str) -> Self {
        let message = error.get("message").and_then(Value::as_str);
        let message = message.filter(|message| !message.is_empty());
        let status = described_status(error);
        let words = message.unwrap_or(unworded);
        let retryable = status.map_or_else(|| names_transient(words), retryable_status);
        let said = match message {
            Some(message) => told(message, MESSAGE_LIMIT),
            None => own(unworded),
        };
        Self::said(vec![said], retryable, status)
    }

    /// The failure of a streamed answer that ended without one event (a
    /// page, a whole JSON body, an end mark alone): final (Python's
    /// `nonempty_stream`).
    #[must_use]
    pub fn no_event() -> Self {
        let said = "The provider's stream carried no event: its body could not be read";
        Self::said(vec![own(said)], false, None)
    }

    /// The failure of a streamed answer past what a provider reads of one,
    /// or that is no stream of events: final, in the reader's words, which
    /// repeat nothing of the answer.
    #[must_use]
    pub fn stream_unread(error: &SseError) -> Self {
        Self::said(vec![own(error.to_string())], false, None)
    }

    /// The failure of a request that got no answer, or whose answer was
    /// lost while it was read: worth a retry.
    #[must_use]
    pub fn transport(error: &reqwest::Error) -> Self {
        let said = if error.is_timeout() {
            "the provider did not answer in time"
        } else {
            "the connection to the provider failed"
        };
        Self::said(vec![own(said)], true, None)
    }

    /// The failure of an answer that says it is JSON and does not read as
    /// JSON, or of a member the reader refuses: final, in the reader's
    /// words, which may quote the answer.
    #[must_use]
    pub fn malformed(error: &serde_json::Error) -> Self {
        Self::said(vec![told(error.to_string(), MESSAGE_LIMIT)], false, None)
    }

    /// The failure of an answer whose body is not the provider's format:
    /// final (Python's `unreadable_response`). `what` says why, in the
    /// provider's own words.
    pub fn unreadable(what: impl Into<Cow<'static, str>>) -> Self {
        let said = "The provider answered with a body it could not read: ";
        Self::said(vec![own(said), own(what)], false, None)
    }

    /// As [`unreadable`](Self::unreadable), naming the `value` the server
    /// wrote that does not read.
    #[must_use]
    pub fn unreadable_value(what: &'static str, value: String) -> Self {
        let said = "The provider answered with a body it could not read: ";
        let value = told(value, NAME_LIMIT);
        Self::said(vec![own(said), own(what), own(": "), value], false, None)
    }

    /// The failure an error object a server answered with in place of a
    /// response describes (a gateway's 200 carrying an error): retried when
    /// the status it names is transient (Python's `error_body_failure`).
    /// Its text is the object's message, or the object itself, as JSON,
    /// when it brings none.
    #[must_use]
    pub fn error_object(body: &Value) -> Self {
        let status = described_status(body);
        let mut objects = error_objects(body, 3).into_iter();
        let message = objects
            .find_map(|object| object.get("message").and_then(Value::as_str))
            .map_or_else(|| text_of(body), str::to_owned);
        let said = told(message, MESSAGE_LIMIT);
        Self::said(vec![said], status.is_some_and(retryable_status), status)
    }
}

/// `body` past the byte order mark it may start with, which Python's JSON
/// reader takes before a document.
#[must_use]
pub fn past_mark(body: &[u8]) -> &[u8] {
    body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body)
}

/// A value as a message carries it: a text as it is, anything else as JSON.
fn text_of(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// Whether a failed request is retried on `status`, on every provider (RFC
/// §6.7): a timeout, a conflict, a rate limit and any server error.
#[must_use]
pub fn retryable_status(status: u16) -> bool {
    matches!(status, 408 | 409 | 429) || (500..600).contains(&status)
}

/// The error objects of a body: what it nests under `error` or `data`,
/// innermost first, then the body itself (Python's `_error_objects`).
fn error_objects(body: &Value, depth: u8) -> Vec<&Value> {
    if depth == 0 || !body.is_object() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for key in ["error", "data"] {
        if let Some(nested) = body.get(key) {
            found.extend(error_objects(nested, depth - 1));
        }
    }
    found.push(body);
    found
}

/// The status an error body describes: its `code`, `type` or `status`,
/// read on the innermost error object first (Python's `_described_status`).
#[must_use]
pub fn described_status(body: &Value) -> Option<u16> {
    let parsed;
    let body = match body.as_str() {
        Some(text) => {
            parsed = serde_json::from_str::<Value>(text).ok()?;
            &parsed
        }
        None => body,
    };
    let objects = error_objects(body, 3).into_iter();
    let named = objects.flat_map(|object| ["code", "type", "status"].map(|key| object.get(key)));
    named.flatten().find_map(status_in)
}

/// `value` as an HTTP error status, if it is one: the number, its digits,
/// or the name of an error's type.
fn status_in(value: &Value) -> Option<u16> {
    let error = |status: u64| {
        u16::try_from(status)
            .ok()
            .filter(|s| (400..600).contains(s))
    };
    match value {
        Value::Number(number) => number.as_u64().and_then(error),
        Value::String(text) if text.bytes().all(|byte| byte.is_ascii_digit()) => {
            text.parse().ok().and_then(error)
        }
        Value::String(text) => ERROR_TYPE_STATUS
            .iter()
            .find(|(name, _)| name == text)
            .map(|(_, status)| *status),
        Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

#[cfg(test)]
mod tests;
