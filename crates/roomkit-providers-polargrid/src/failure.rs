//! What a failed request reads as on PolarGrid: how the body of a refused
//! request is read, and the status it stands for, as polargrid-sdk words
//! it (`_parse_error_response`, `create_error_from_response`) and RoomKit
//! reads its classes (`_status_for`, `provider_error`).
//!
//! A failure is a [`Failure`] of `roomkit-providers-client`, which keeps
//! the provider's own words apart from what a server or a reader said. It
//! leaves the provider through that crate's `Exit`, where what was said is
//! cleaned of the configuration's secrets, put on one line and bounded.

pub(crate) use roomkit_providers_client::failure::{Failure, past_mark};
use roomkit_providers_client::failure::{
    MESSAGE_LIMIT, names_transient, own, retryable_status, told,
};
use serde_json::Value;

/// A refused request: the status the SDK's class stands for (a 403 is read
/// as a 401, the SDK folding both into one class; a status it has no class
/// for stands for none), retryable as that status is, else by its words;
/// the body's `message`, else its `detail`, else the body. Python reads no
/// overflow of the model's context from PolarGrid's errors.
pub(crate) fn status(status: u16, body: &[u8]) -> Failure {
    let message = message(status, body);
    let read = match status {
        401 | 403 => Some(401),
        400 | 402 | 404 | 429 | 500.. => Some(status),
        _ => None,
    };
    let retryable = read.map_or_else(|| names_transient(&message), retryable_status);
    let said = vec![
        own(format!("the provider answered HTTP {status}: ")),
        told(message, MESSAGE_LIMIT),
    ];
    Failure::said(said, retryable, read)
}

/// What the body of a refused request says, as the SDK reads it.
fn message(status: u16, body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body).into_owned();
    let Ok(parsed) = serde_json::from_slice::<Value>(past_mark(body)) else {
        return if text.is_empty() {
            format!("HTTP {status}")
        } else {
            text
        };
    };
    let stated = |name: &str| {
        let value = parsed.get(name)?;
        let truthy = roomkit_providers_client::chat::truthy(value);
        truthy.then(|| {
            value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned)
        })
    };
    stated("message")
        .or_else(|| stated("detail"))
        .unwrap_or(text)
}

/// A request the SDK refuses before sending it, in its words.
pub(crate) fn refused_by_the_sdk(said: &'static str) -> Failure {
    Failure::said(vec![own(said)], false, None)
}

/// An answer the SDK does not read as one.
pub(crate) fn unread(error: &serde_json::Error) -> Failure {
    Failure::malformed(error)
}

// The SDK quotes its own request id after these words, so a replay does not
// compare them. Python: polargrid-sdk's _parse_error_response.
#[cfg(test)]
mod tests {
    use super::message;

    #[test]
    fn a_refusal_says_its_message_else_its_detail_else_its_body() {
        let said = [
            (r#"{"message": "invalid", "detail": "too long"}"#, "invalid"),
            (r#"{"message": "", "detail": "too long"}"#, "too long"),
            (r#"{"detail": "too long"}"#, "too long"),
            (r#"{"code": "e"}"#, r#"{"code": "e"}"#),
            ("<html>Bad Gateway</html>", "<html>Bad Gateway</html>"),
            ("", "HTTP 502"),
        ];
        for (body, words) in said {
            assert_eq!(message(502, body.as_bytes()), words, "{body}");
        }
    }
}
