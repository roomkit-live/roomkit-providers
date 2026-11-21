//! What a failed request reads as: Python's `provider_error` and its
//! helpers, with what a failure echoes of a server's answer bounded and
//! cleaned of the key.

use roomkit_providers_core::{ProviderError, ProviderErrorOptions};
use roomkit_providers_http::SseError;
use serde_json::Value;

use self::words::names_transient;

mod words;

/// The most characters of a server's answer a failure's message carries.
pub(crate) const MESSAGE_LIMIT: usize = 500;

/// The most characters of an error's type or code a failure's message
/// carries.
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

/// Who fails: the provider's name, and the key no message may carry.
#[derive(Clone, Copy)]
pub(crate) struct Failing<'p> {
    pub(crate) provider: &'p str,
    pub(crate) secret: &'p str,
}

impl std::fmt::Debug for Failing<'_> {
    /// The provider's name: never the key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Failing")
            .field("provider", &self.provider)
            .finish_non_exhaustive()
    }
}

impl Failing<'_> {
    fn error(self, message: &str, retryable: bool, status: Option<u16>) -> ProviderError {
        let options = ProviderErrorOptions {
            retryable,
            provider: self.provider.to_owned(),
            status_code: status,
            ..ProviderErrorOptions::default()
        };
        ProviderError::new(cleaned(message, self.secret, MESSAGE_LIMIT), options)
    }

    /// The failure of an answer whose status is no success, retried when
    /// the status is transient: `the provider answered HTTP 401 (type,
    /// code): message`. Of the body it carries what an error object says
    /// of itself and nothing else: its message, its type and its code,
    /// read under `error` or at the top level, each on one line, bounded
    /// and without the key. A body that is no JSON is not echoed: the
    /// message is then the status's reason. A redirect is a failure: it
    /// is not followed.
    pub(crate) fn status(self, status: u16, body: &[u8]) -> ProviderError {
        if (300..400).contains(&status) {
            let message =
                format!("the endpoint answered with a redirect ({status}), which is not followed");
            return self.error(&message, false, Some(status));
        }
        let parsed: Option<Value> = serde_json::from_slice(body).ok();
        let said = |name: &str| said_of(parsed.as_ref()?, name);
        let named = |name: &str| {
            let name = cleaned(&said(name)?, self.secret, NAME_LIMIT);
            Some(name).filter(|name| !name.is_empty())
        };
        let (kind, code) = (named("type"), named("code"));
        let detail = match (&kind, &code) {
            (Some(kind), Some(code)) => format!(" ({kind}, {code})"),
            (Some(named), None) | (None, Some(named)) => format!(" ({named})"),
            (None, None) => String::new(),
        };
        let bare = || parsed.as_ref()?.get("error")?.as_str().map(str::to_owned);
        let reason = || {
            reqwest::StatusCode::from_u16(status)
                .ok()?
                .canonical_reason()
        };
        let message = said("message")
            .or_else(bare)
            .unwrap_or_else(|| reason().unwrap_or("no message").to_owned());
        let message = format!("the provider answered HTTP {status}{detail}: {message}");
        let mut error = self.error(&message, retryable_status(status), Some(status));
        // The error object's code is a first-hand fact, unlike its wording.
        if code.as_deref() == Some("context_length_exceeded") {
            error.context_overflow = Some(true);
        }
        error
    }

    /// The failure of an error a server wrote inside its stream, in place
    /// of an event: the status the error names decides whether it is
    /// retried, else its words (Python's `provider_error`). Its message is
    /// the error's own, or the words of Python's client for an error that
    /// brings none.
    pub(crate) fn stream_error(self, error: &Value) -> ProviderError {
        let message = error.get("message").and_then(Value::as_str);
        let message = message.filter(|message| !message.is_empty());
        let message = message.unwrap_or(STREAM_ERROR);
        let status = described_status(error);
        let retryable = status.map_or_else(|| names_transient(message), retryable_status);
        self.error(message, retryable, status)
    }

    /// The failure of a streamed answer that ended without one event (a
    /// page, a whole JSON body, the end mark alone): final (Python's
    /// `nonempty_stream`).
    pub(crate) fn no_event(self) -> ProviderError {
        let message = "The provider's stream carried no event: its body could not be read";
        self.error(message, false, None)
    }

    /// The failure of a streamed answer past what the provider reads of
    /// one, or that is no stream of events: final.
    pub(crate) fn stream_unread(self, error: &SseError) -> ProviderError {
        self.error(&error.to_string(), false, None)
    }

    /// The failure of a request that got no answer, or whose answer was
    /// lost while it was read: worth a retry.
    pub(crate) fn transport(self, error: &reqwest::Error) -> ProviderError {
        let message = if error.is_timeout() {
            "the provider did not answer in time"
        } else {
            "the connection to the provider failed"
        };
        self.error(message, true, None)
    }

    /// The failure of an answer that says it is JSON and does not read as
    /// JSON: final, in the words of the JSON reader.
    pub(crate) fn malformed(self, error: &serde_json::Error) -> ProviderError {
        self.error(&error.to_string(), false, None)
    }

    /// The failure of an answer whose body is not the provider's format: final.
    pub(crate) fn unreadable(self, what: &str) -> ProviderError {
        let message = format!("The provider answered with a body it could not read: {what}");
        self.error(&message, false, None)
    }

    /// The failure an error object a server answered with in place of a
    /// response describes (a gateway's 200 carrying an error): retried when
    /// the status it names is transient (Python's `error_body_failure`).
    pub(crate) fn error_object(self, body: &Value) -> ProviderError {
        let status = described_status(body);
        let said = error_objects(body, 3).into_iter();
        let mut said = said;
        let message = said
            .find_map(|object| object.get("message").and_then(Value::as_str))
            .map_or_else(|| text_of(body), str::to_owned);
        self.error(&message, status.is_some_and(retryable_status), status)
    }
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

/// `text` as an error carries it: `secret` removed wherever it appears, on
/// one line, within `limit` characters.
pub(crate) fn cleaned(text: &str, secret: &str, limit: usize) -> String {
    let removed = if secret.is_empty() {
        text.to_owned()
    } else {
        text.replace(secret, "[removed]")
    };
    let line = removed.split_whitespace().collect::<Vec<_>>().join(" ");
    line.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const FAILING: Failing<'static> = Failing {
        provider: "openai",
        secret: "placeholder-not-a-key",
    };

    #[test]
    fn a_message_never_carries_the_key_and_is_bounded() {
        let echoed = json!({"error": {
            "message": "Incorrect key: placeholder-not-a-key.",
            "type": "placeholder-not-a-key",
            "code": "placeholder-not-a-key",
        }});
        let error = FAILING.status(401, echoed.to_string().as_bytes());
        assert_eq!(
            error.message,
            "the provider answered HTTP 401 ([removed], [removed]): Incorrect key: [removed]."
        );
        let long = json!({"error": {
            "message": "x".repeat(4 * MESSAGE_LIMIT),
            "type": "t".repeat(4 * NAME_LIMIT),
        }});
        let long = FAILING.status(400, long.to_string().as_bytes());
        assert_eq!(long.message.chars().count(), MESSAGE_LIMIT);
        let named = format!(
            "the provider answered HTTP 400 ({}): xxx",
            "t".repeat(NAME_LIMIT)
        );
        assert!(long.message.starts_with(&named), "{}", long.message);
        let lines = json!({"error": {"message": " model\n not  loaded \n"}});
        assert_eq!(
            FAILING.status(400, lines.to_string().as_bytes()).message,
            "the provider answered HTTP 400: model not loaded"
        );
    }

    // Source: src/wire.rs::tests::an_error_answer_gives_the_provider_s_type_code_and_message
    // and ::an_error_answer_that_is_no_json_gives_the_status_s_reason_only
    #[test]
    fn an_error_answer_gives_its_type_code_and_message_or_the_status_s_reason() {
        let said = |status: u16, body: &str| FAILING.status(status, body.as_bytes()).message;
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

    // What a host sees of a refused request: the error object's own words,
    // and nothing of a body that is no JSON.
    #[test]
    fn a_status_failure_says_what_the_error_object_says_and_echoes_no_other_body() {
        let said = |status: u16, body: &str| FAILING.status(status, body.as_bytes()).message;
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
            said(503, r#"{"detail": "busy"}"#),
            "the provider answered HTTP 503: Service Unavailable"
        );
    }

    #[test]
    fn a_status_decides_whether_a_failure_is_retried() {
        for (status, retryable) in [(400, false), (408, true), (409, true), (429, true)] {
            assert_eq!(
                FAILING.status(status, b"{}").retryable,
                retryable,
                "{status}"
            );
        }
        assert!(FAILING.status(529, b"").retryable);
        let redirect = FAILING.status(307, b"");
        assert_eq!(
            (redirect.retryable, redirect.status_code),
            (false, Some(307))
        );
        let overflow = json!({"error": {"code": "context_length_exceeded"}}).to_string();
        assert_eq!(
            FAILING.status(400, overflow.as_bytes()).context_overflow,
            Some(true)
        );
        assert_eq!(FAILING.status(400, b"{}").context_overflow, None);
    }

    // Python: provider_error on the error a stream carried: the status it
    // names decides, else its words.
    #[test]
    fn an_error_inside_a_stream_reads_as_its_status_or_its_words() {
        let read = |error: Value| {
            let error = FAILING.stream_error(&error);
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
            named(None, false, "key [removed]")
        );
    }

    #[test]
    fn an_error_object_reads_as_the_status_it_names() {
        let read = |body: Value| {
            let error = FAILING.error_object(&body);
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
