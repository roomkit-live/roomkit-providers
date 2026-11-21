//! What a failed request reads as: Python's `provider_error` and its
//! helpers.
//!
//! A failure is built here as a [`Failure`], which keeps the provider's own
//! words apart from what a server, a reader or the contract said. It leaves
//! the provider through [`exit::Exit::leave`], the one place the contract's
//! error is made: there what was said is cleaned of the configuration's
//! secrets, put on one line and bounded.

use std::borrow::Cow;
use std::fmt;

use roomkit_providers_http::SseError;
use roomkit_providers_llm::{ProviderError, is_context_overflow_message};
use serde_json::Value;

use self::words::names_transient;

pub(crate) mod exit;
#[cfg(test)]
mod exit_tests;
mod words;

/// The most characters of a failure's text, and of what it repeats of a
/// server's answer.
pub(crate) const MESSAGE_LIMIT: usize = 500;

/// The most characters of an error's type or code, or of a value a reader
/// refused, a failure's text carries.
const NAME_LIMIT: usize = 64;

/// What Python's client says of an error inside a stream that brings no
/// message of its own.
const STREAM_ERROR: &str = "An error occurred during streaming";

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

/// A piece of a failure's text.
#[derive(Clone)]
enum Said {
    /// The provider's own words: never searched for a secret.
    Own(Cow<'static, str>),
    /// What a server or a reader said, and the most characters of it the
    /// text carries: cleaned when the failure leaves.
    Told(String, usize),
}

/// The provider's own words.
fn own(text: impl Into<Cow<'static, str>>) -> Said {
    Said::Own(text.into())
}

/// How a failure was built.
#[derive(Clone)]
enum Built {
    /// By this module, its text in pieces.
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

/// A failure inside the provider. It becomes the contract's error only
/// where it leaves ([`exit::Exit::leave`]).
#[derive(Clone)]
pub(crate) struct Failure(Built);

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

impl Failure {
    fn said(said: Vec<Said>, retryable: bool, status: Option<u16>) -> Self {
        Self(Built::Here {
            said,
            retryable,
            status,
            context_overflow: None,
            before_request: false,
        })
    }

    /// A failure before any request, final: `said`, the provider's own
    /// words.
    pub(crate) fn before_request(said: impl Into<Cow<'static, str>>) -> Self {
        Self(Built::Here {
            said: vec![own(said)],
            retryable: false,
            status: None,
            context_overflow: None,
            before_request: true,
        })
    }

    /// The failure of an answer whose status is no success, retried when
    /// the status is transient: `the provider answered HTTP 401 (type,
    /// code): message`. Of the body it carries what an error object says
    /// of itself and nothing else: its message, its type and its code,
    /// read under `error` or at the top level, and, where it states no
    /// message, the text it words its refusal with under `detail`. A body
    /// that is no JSON is not echoed: the text is then the status's
    /// reason. Whether the
    /// request overflowed the model's context is read from the whole body,
    /// echoed or not: the code that says so, or the words. A redirect is a
    /// failure: it is not followed.
    pub(crate) fn status(status: u16, body: &[u8]) -> Self {
        if (300..400).contains(&status) {
            let said =
                format!("the endpoint answered with a redirect ({status}), which is not followed");
            return Self::said(vec![own(said)], false, Some(status));
        }
        let parsed: Option<Value> = serde_json::from_slice(past_mark(body)).ok();
        let stated = |name: &str| {
            let stated = said_of(parsed.as_ref()?, name)?;
            Some(stated).filter(|stated| !exit::one_line(stated).is_empty())
        };
        let mut said = vec![own(format!("the provider answered HTTP {status}"))];
        let (kind, code) = (stated("type"), stated("code"));
        // The code is compared as the server wrote it.
        let by_code = code.as_deref() == Some("context_length_exceeded");
        let name = |name: String| Said::Told(name, NAME_LIMIT);
        match (kind, code) {
            (Some(kind), Some(code)) => {
                said.extend([own(" ("), name(kind), own(", "), name(code), own(")")]);
            }
            (Some(named), None) | (None, Some(named)) => {
                said.extend([own(" ("), name(named), own(")")]);
            }
            (None, None) => {}
        }
        said.push(own(": "));
        let worded = |name: &str| {
            let worded = parsed.as_ref()?.get(name)?.as_str()?;
            Some(worded.to_owned()).filter(|worded| !exit::one_line(worded).is_empty())
        };
        let bare = || parsed.as_ref()?.get("error")?.as_str().map(str::to_owned);
        let reason = || {
            let status = reqwest::StatusCode::from_u16(status).ok()?;
            status.canonical_reason()
        };
        let message = stated("message").or_else(bare);
        said.push(match message.or_else(|| worded("detail")) {
            Some(message) => Said::Told(message, MESSAGE_LIMIT),
            None => own(reason().unwrap_or("no message")),
        });
        let mut failure = Self::said(said, retryable_status(status), Some(status));
        // Everything the server said is read for an overflow, not only
        // what is echoed.
        let by_words = is_context_overflow_message(&String::from_utf8_lossy(body));
        if let Built::Here {
            context_overflow, ..
        } = &mut failure.0
            && (by_code || by_words)
        {
            *context_overflow = Some(true);
        }
        failure
    }

    /// The failure of an error a server wrote inside its stream, in place
    /// of an event: the status the error names decides whether it is
    /// retried, else its words (Python's `provider_error`). Its text is
    /// the error's own message, or the words of Python's client for an
    /// error that brings none.
    pub(crate) fn stream_error(error: &Value) -> Self {
        let message = error.get("message").and_then(Value::as_str);
        let message = message.filter(|message| !message.is_empty());
        let status = described_status(error);
        let words = message.unwrap_or(STREAM_ERROR);
        let retryable = status.map_or_else(|| names_transient(words), retryable_status);
        let said = match message {
            Some(message) => Said::Told(message.to_owned(), MESSAGE_LIMIT),
            None => own(STREAM_ERROR),
        };
        Self::said(vec![said], retryable, status)
    }

    /// The failure of a streamed answer that ended without one event (a
    /// page, a whole JSON body, the end mark alone): final (Python's
    /// `nonempty_stream`).
    pub(crate) fn no_event() -> Self {
        let said = "The provider's stream carried no event: its body could not be read";
        Self::said(vec![own(said)], false, None)
    }

    /// The failure of a streamed answer that ended with neither a finish
    /// reason nor its end mark: it was cut, as one whose connection is
    /// lost, and is worth a retry.
    pub(crate) fn cut_short() -> Self {
        let said = "the provider's stream ended with neither a finish reason nor its end mark";
        Self::said(vec![own(said)], true, None)
    }

    /// The failure of a streamed answer past what the provider reads of
    /// one, or that is no stream of events: final, in the reader's words,
    /// which repeat nothing of the answer.
    pub(crate) fn stream_unread(error: &SseError) -> Self {
        Self::said(vec![own(error.to_string())], false, None)
    }

    /// The failure of a request that got no answer, or whose answer was
    /// lost while it was read: worth a retry.
    pub(crate) fn transport(error: &reqwest::Error) -> Self {
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
    pub(crate) fn malformed(error: &serde_json::Error) -> Self {
        let said = Said::Told(error.to_string(), MESSAGE_LIMIT);
        Self::said(vec![said], false, None)
    }

    /// The failure of an answer whose body is not the provider's format:
    /// final. `what` says why, in the provider's own words.
    pub(crate) fn unreadable(what: impl Into<Cow<'static, str>>) -> Self {
        let said = "The provider answered with a body it could not read: ";
        Self::said(vec![own(said), own(what)], false, None)
    }

    /// The failure of an answer of status 200 that is no chat completion:
    /// final. Where its `body` is JSON and words why, under `message` or
    /// else under `detail`, as a text, the failure carries those words.
    pub(crate) fn no_completion(body: Option<&Value>) -> Self {
        let worded = |name: &str| {
            let worded = body?.get(name)?.as_str()?;
            Some(worded.to_owned()).filter(|worded| !exit::one_line(worded).is_empty())
        };
        let said = "The provider answered with a body it could not read: not a chat completion";
        let mut said = vec![own(said)];
        if let Some(words) = worded("message").or_else(|| worded("detail")) {
            said.extend([own(": "), Said::Told(words, MESSAGE_LIMIT)]);
        }
        Self::said(said, false, None)
    }

    /// As [`unreadable`](Self::unreadable), naming the `value` the server
    /// wrote that does not read.
    pub(crate) fn unreadable_value(what: &'static str, value: String) -> Self {
        let said = "The provider answered with a body it could not read: ";
        let value = Said::Told(value, NAME_LIMIT);
        Self::said(vec![own(said), own(what), own(": "), value], false, None)
    }

    /// The failure an error object a server answered with in place of a
    /// response describes (a gateway's 200 carrying an error): retried when
    /// the status it names is transient (Python's `error_body_failure`).
    /// Its text is the object's message, or the object itself, as JSON,
    /// when it brings none.
    pub(crate) fn error_object(body: &Value) -> Self {
        let status = described_status(body);
        let mut objects = error_objects(body, 3).into_iter();
        let message = objects
            .find_map(|object| object.get("message").and_then(Value::as_str))
            .map_or_else(|| text_of(body), str::to_owned);
        let said = Said::Told(message, MESSAGE_LIMIT);
        Self::said(vec![said], status.is_some_and(retryable_status), status)
    }
}

/// `body` past the byte order mark it may start with, which Python's JSON
/// reader takes before a document.
pub(crate) fn past_mark(body: &[u8]) -> &[u8] {
    body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body)
}

/// What the error object of `body` says under `name`: read in its `error`
/// object, then at the top level, a text or a number.
fn said_of(body: &Value, name: &str) -> Option<String> {
    let error = body.get("error").filter(|error| error.is_object());
    let found = error.and_then(|error| error.get(name));
    match found.or_else(|| body.get(name))? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

/// A value as a message carries it: a text as it is, anything else as JSON.
fn text_of(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// Whether a failed request is retried on `status`, on every provider (RFC
/// §6.7): a timeout, a conflict, a rate limit and any server error.
pub(crate) fn retryable_status(status: u16) -> bool {
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
fn described_status(body: &Value) -> Option<u16> {
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
mod tests {
    use serde_json::json;

    use super::*;

    use super::exit::test_exit;
    use roomkit_providers_llm::ProviderError;

    /// The failure of a status, as it leaves a provider whose key is the
    /// placeholder.
    fn refusal_of(status: u16, body: &[u8]) -> ProviderError {
        test_exit().leave(&Failure::status(status, body))
    }

    #[test]
    fn a_message_never_carries_the_key_and_is_bounded() {
        let echoed = json!({"error": {
            "message": "Incorrect key: placeholder-not-a-key.",
            "type": "placeholder-not-a-key",
            "code": "placeholder-not-a-key",
        }});
        let error = refusal_of(401, echoed.to_string().as_bytes());
        assert_eq!(
            error.message,
            "the provider answered HTTP 401 ([***], [***]): Incorrect key: [***]."
        );
        let long = json!({"error": {
            "message": "x".repeat(4 * MESSAGE_LIMIT),
            "type": "t".repeat(4 * NAME_LIMIT),
        }});
        let long = refusal_of(400, long.to_string().as_bytes());
        assert_eq!(long.message.chars().count(), MESSAGE_LIMIT);
        let named = format!(
            "the provider answered HTTP 400 ({}): xxx",
            "t".repeat(NAME_LIMIT)
        );
        assert!(long.message.starts_with(&named), "{}", long.message);
        let lines = json!({"error": {"message": " model\n not  loaded \n"}});
        assert_eq!(
            refusal_of(400, lines.to_string().as_bytes()).message,
            "the provider answered HTTP 400: model not loaded"
        );
    }

    // Source: src/wire.rs::tests::an_error_answer_gives_the_provider_s_type_code_and_message
    // and ::an_error_answer_that_is_no_json_gives_the_status_s_reason_only
    #[test]
    fn an_error_answer_gives_its_type_code_and_message_or_the_status_s_reason() {
        let said = |status: u16, body: &str| refusal_of(status, body.as_bytes()).message;
        let own = r#"{"error": {"message": "Incorrect API key", "type": "invalid_request_error", "code": "invalid_api_key"}}"#;
        let flat = r#"{"object": "error", "message": "The model does not exist.", "type": "NotFoundError", "code": 404}"#;
        assert_eq!(
            said(401, own),
            "the provider answered HTTP 401 (invalid_request_error, invalid_api_key): \
             Incorrect API key"
        );
        assert_eq!(
            said(404, flat),
            "the provider answered HTTP 404 (NotFoundError, 404): The model does not exist."
        );
        assert_eq!(
            said(502, "<html>echo: placeholder-not-a-key</html>"),
            "the provider answered HTTP 502: Bad Gateway"
        );
    }

    // A server that words its refusal under `detail`, with a code or none:
    // its text is the message, bounded and cleaned as any other. A message
    // the body states comes first, and a detail that is no text is not read.
    #[test]
    fn a_detail_is_the_message_of_a_body_that_states_no_other() {
        let said = |status: u16, body: &str| refusal_of(status, body.as_bytes()).message;
        let coded = r#"{"detail": "Model m is not deployed here", "code": "model_not_found"}"#;
        assert_eq!(
            said(404, coded),
            "the provider answered HTTP 404 (model_not_found): Model m is not deployed here"
        );
        assert_eq!(
            said(403, r#"{"detail": " Not\n authenticated "}"#),
            "the provider answered HTTP 403: Not authenticated"
        );
        assert_eq!(
            said(401, r#"{"detail": "Wrong key: placeholder-not-a-key"}"#),
            "the provider answered HTTP 401: Wrong key: [***]"
        );
        let long = json!({"detail": "x".repeat(4 * MESSAGE_LIMIT)}).to_string();
        assert_eq!(said(400, &long).chars().count(), MESSAGE_LIMIT);
        let both = r#"{"message": "The model is loading.", "detail": "Try again."}"#;
        assert_eq!(
            said(503, both),
            "the provider answered HTTP 503: The model is loading."
        );
        for unread in [
            r#"{"detail": [{"msg": "field required"}]}"#,
            r#"{"detail": ""}"#,
        ] {
            assert_eq!(
                said(422, unread),
                "the provider answered HTTP 422: Unprocessable Entity"
            );
        }
    }

    // What a host sees of a refused request: the error object's own words,
    // and nothing of a body that is no JSON.
    #[test]
    fn a_status_failure_says_what_the_error_object_says_and_echoes_no_other_body() {
        let said = |status: u16, body: &str| refusal_of(status, body.as_bytes()).message;
        let object = json!({"error": {
            "message": "Incorrect API key provided.",
            "type": "invalid_request_error",
            "param": null,
            "code": "invalid_api_key",
        }});
        assert_eq!(
            said(401, &object.to_string()),
            "the provider answered HTTP 401 (invalid_request_error, invalid_api_key): \
             Incorrect API key provided."
        );
        // A server that writes its error at the top level, and a code that
        // is a number.
        let flat = json!({"message": "The model is not loaded.", "code": 404});
        assert_eq!(
            said(404, &flat.to_string()),
            "the provider answered HTTP 404 (404): The model is not loaded."
        );
        let bare = json!({"error": "model not found"});
        assert_eq!(
            said(404, &bare.to_string()),
            "the provider answered HTTP 404: model not found"
        );
        let page = "<html><body><h1>502 Bad Gateway</h1>nginx at 10.0.0.7</body></html>";
        assert_eq!(
            said(502, page),
            "the provider answered HTTP 502: Bad Gateway"
        );
        assert_eq!(
            said(400, "the model is not loaded"),
            "the provider answered HTTP 400: Bad Request"
        );
        assert_eq!(
            said(500, ""),
            "the provider answered HTTP 500: Internal Server Error"
        );
        assert_eq!(said(529, ""), "the provider answered HTTP 529: no message");
        // An object that says nothing of an error.
        assert_eq!(
            said(503, r#"{"state": "busy"}"#),
            "the provider answered HTTP 503: Service Unavailable"
        );
    }

    #[test]
    fn a_status_decides_whether_a_failure_is_retried() {
        for (status, retryable) in [(400, false), (408, true), (409, true), (429, true)] {
            assert_eq!(refusal_of(status, b"{}").retryable, retryable, "{status}");
        }
        assert!(refusal_of(529, b"").retryable);
        let redirect = refusal_of(307, b"");
        assert_eq!(
            (redirect.retryable, redirect.status_code),
            (false, Some(307))
        );
        let overflow = json!({"error": {"code": "context_length_exceeded"}}).to_string();
        assert_eq!(
            refusal_of(400, overflow.as_bytes()).context_overflow,
            Some(true)
        );
        assert_eq!(refusal_of(400, b"{}").context_overflow, None);
    }

    // The bounds are exact: a text of 500 characters is whole, one more is
    // cut; a type or a code of 64 characters is whole, one more is cut.
    #[test]
    fn a_failure_s_text_is_bounded_exactly() {
        let own = "the provider answered HTTP 400: ";
        let said = |message: &str| {
            let body = json!({"error": {"message": message}}).to_string();
            refusal_of(400, body.as_bytes()).message
        };
        let fits = "m".repeat(500 - own.len());
        assert_eq!(said(&fits), format!("{own}{fits}"));
        assert_eq!(said(&fits).chars().count(), 500);
        assert_eq!(said(&format!("{fits}!")), format!("{own}{fits}"));
        // The bound counts characters, not bytes.
        let wide = "\u{e9}".repeat(500 - own.len());
        assert_eq!(said(&format!("{wide}!")), format!("{own}{wide}"));
        let named = |kind: &str| {
            let body = json!({"error": {"message": "No.", "type": kind}}).to_string();
            refusal_of(400, body.as_bytes()).message
        };
        let fits = "t".repeat(64);
        let whole = format!("the provider answered HTTP 400 ({fits}): No.");
        assert_eq!(named(&fits), whole);
        assert_eq!(named(&format!("{fits}!")), whole);
    }

    // Python: provider_error on the error a stream carried: the status it
    // names decides, else its words.
    #[test]
    fn an_error_inside_a_stream_reads_as_its_status_or_its_words() {
        let read = |error: Value| {
            let error = test_exit().leave(&Failure::stream_error(&error));
            (error.status_code, error.retryable, error.message)
        };
        let named = |status: Option<u16>, retryable: bool, message: &str| {
            (status, retryable, message.to_owned())
        };
        assert_eq!(
            read(json!({"message": "The server had an error.", "type": "server_error"})),
            named(Some(500), true, "The server had an error.")
        );
        assert_eq!(
            read(json!({"message": "No such model.", "code": 404})),
            named(Some(404), false, "No such model.")
        );
        assert_eq!(
            read(json!({"message": "Rate limit reached"})),
            named(None, true, "Rate limit reached")
        );
        assert_eq!(
            read(json!({"message": "The token limit was reached."})),
            named(None, false, "The token limit was reached.")
        );
        assert_eq!(
            read(json!("rate limit")),
            named(None, false, "An error occurred during streaming")
        );
        assert_eq!(
            read(json!({"message": "key placeholder-not-a-key"})),
            named(None, false, "key [***]")
        );
    }

    #[test]
    fn an_error_object_reads_as_the_status_it_names() {
        let read = |body: Value| {
            let error = test_exit().leave(&Failure::error_object(&body));
            (error.status_code, error.retryable, error.message)
        };
        let named = |status: Option<u16>, retryable: bool, message: &str| {
            (status, retryable, message.to_owned())
        };
        assert_eq!(
            read(json!({"message": "Overloaded.", "type": "overloaded_error"})),
            named(Some(529), true, "Overloaded.")
        );
        assert_eq!(
            read(json!({"message": "Later.", "code": 503})),
            named(Some(503), true, "Later.")
        );
        assert_eq!(
            read(json!({"message": "Slow.", "code": "429"})),
            named(Some(429), true, "Slow.")
        );
        assert_eq!(
            read(json!({"message": "Quota.", "status": "RESOURCE_EXHAUSTED"})),
            named(Some(429), true, "Quota.")
        );
        assert_eq!(
            read(json!("the model is loading")),
            named(None, false, "the model is loading")
        );
        assert_eq!(
            read(json!({"code": true, "type": 200})),
            named(None, false, r#"{"code":true,"type":200}"#)
        );
        let nested = json!({"message": "outer", "error": {"message": "inner", "code": 404}});
        assert_eq!(read(nested), named(Some(404), false, "inner"));
    }
}
