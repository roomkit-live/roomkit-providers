//! What a failed request reads as: Python's `provider_error` and its
//! helpers, with what a failure echoes of a server's answer bounded and
//! cleaned of the key.

use roomkit_providers_core::{ProviderError, ProviderErrorOptions};
use serde_json::Value;

/// The most characters of a server's answer a failure's message carries.
pub(crate) const MESSAGE_LIMIT: usize = 500;

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

    /// The failure of an answer whose status is no success: retried when
    /// the status is transient, with the body the server answered, as
    /// Python's `status_error_text` keeps it. A redirect is one: it is not
    /// followed.
    pub(crate) fn status(self, status: u16, body: &[u8]) -> ProviderError {
        if (300..400).contains(&status) {
            let message =
                format!("the endpoint answered with a redirect ({status}), which is not followed");
            return self.error(&message, false, Some(status));
        }
        let text = String::from_utf8_lossy(body);
        let text = text.trim();
        let parsed = serde_json::from_str::<Value>(text).ok();
        let message = match &parsed {
            _ if text.is_empty() => format!("Error code: {status}"),
            Some(body) => format!("Error code: {status} - {body}"),
            None => text.to_owned(),
        };
        let mut error = self.error(&message, retryable_status(status), Some(status));
        // The error object's code is a first-hand fact, unlike its wording.
        let described = parsed
            .as_ref()
            .map(|body| body.get("error").unwrap_or(body));
        let code = described.and_then(|error| error.get("code"));
        if code.and_then(Value::as_str) == Some("context_length_exceeded") {
            error.context_overflow = Some(true);
        }
        error
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
        let echoed = json!({"error": {"message": "Incorrect key: placeholder-not-a-key."}});
        let error = FAILING.status(401, echoed.to_string().as_bytes());
        assert!(!error.message.contains("placeholder"), "{}", error.message);
        assert!(error.message.contains("[removed]"));
        let long = "x".repeat(4 * MESSAGE_LIMIT);
        assert_eq!(
            FAILING.status(400, long.as_bytes()).message.chars().count(),
            MESSAGE_LIMIT
        );
        let lines = FAILING.status(400, b" model\n not  loaded \n");
        assert_eq!(lines.message, "model not loaded");
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
        assert_eq!(FAILING.status(500, b"").message, "Error code: 500");
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
