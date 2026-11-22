//! What a failed request reads as on Anthropic's wire: how the body of a
//! refused request is read, and the failures of a message in the provider's
//! own words.
//!
//! A failure is a [`Failure`] of `roomkit-providers-client`, which keeps
//! the provider's own words apart from what a server, a reader or the
//! contract said, and words the failures every provider shares. It leaves
//! the provider through that crate's `Exit`, the one place the contract's
//! error is made: there what was said is cleaned of the configuration's
//! secrets, put on one line and bounded.

use roomkit_ai::is_context_overflow_message;
pub(crate) use roomkit_providers_client::failure::{Failure, past_mark};
use roomkit_providers_client::failure::{
    MESSAGE_LIMIT, Stated, one_line, own, retryable_status, told,
};
use serde_json::Value;

/// The failure of an answer whose status is neither a success nor a
/// redirect. Its text is the message of Anthropic's error object, read
/// under `error`, and nothing else: a host reads in it what the API says,
/// from its first word (`messages.2.content.1: Invalid signature…`), and
/// the status is the failure's own. A body that states no message is not
/// echoed: the text is then `the provider answered HTTP 502: Bad Gateway`,
/// the status and its reason, after the type when the body states one.
/// Whether the request overflowed the model's context is read from the
/// whole body, echoed or not, by its words: Anthropic names no code for it.
pub(crate) fn status(status: u16, body: &[u8]) -> Failure {
    let parsed: Option<Value> = serde_json::from_slice(past_mark(body)).ok();
    let stated = |name: &str| {
        let stated = parsed.as_ref()?.get("error")?.get(name)?.as_str()?;
        Some(stated.to_owned()).filter(|stated| !one_line(stated).is_empty())
    };
    let Some(message) = stated("message") else {
        let stated = Stated {
            kind: stated("type"),
            code: None,
            message: None,
            overflow: false,
        };
        return Failure::refused(status, body, stated);
    };
    let said = vec![told(message, MESSAGE_LIMIT)];
    let failure = Failure::said(said, retryable_status(status), Some(status));
    // Everything the server said is read for an overflow, not only what is
    // echoed.
    if is_context_overflow_message(&String::from_utf8_lossy(body)) {
        failure.overflowing()
    } else {
        failure
    }
}

/// The failure of a streamed message that ended with neither a stop reason
/// nor the event that ends it: it was cut, as one whose connection is lost,
/// and is worth a retry.
pub(crate) fn cut_short() -> Failure {
    let said = "the provider's stream ended with neither a stop reason nor the end of its message";
    Failure::said(vec![own(said)], true, None)
}

/// The failure of a request that has no key to carry: neither its own, nor
/// the configuration's, nor one an extra header brings. Final, and nothing
/// was sent.
pub(crate) fn no_key() -> Failure {
    let said = "the request carries no key: the configuration holds none, and neither does the \
                request";
    Failure::said(vec![own(said)], false, None)
}

/// The failure of a request whose own key cannot go in a header: before any
/// request, final. It repeats nothing of the key.
pub(crate) fn no_header_for_the_key() -> Failure {
    Failure::before_request("the key of the request cannot go in a header")
}

/// The failure of an answer that gave no event of a message: what the error
/// object it is says, when it is a JSON document that holds one, else that
/// no event came.
pub(crate) fn no_event(answer: Option<&Value>) -> Failure {
    let error = answer.filter(|answer| holds_an_error(answer));
    error.map_or_else(Failure::no_event, Failure::error_object)
}

/// The failure of a model listing that is no page of models: final.
pub(crate) fn no_listing() -> Failure {
    Failure::unreadable("the model listing is no page of models with their ids")
}

/// Whether `answer`, a JSON document, holds an error: an `error` member
/// that says something.
pub(crate) fn holds_an_error(answer: &Value) -> bool {
    answer.get("error").is_some_and(said_something)
}

/// Whether Python reads `value` as true: a member that is null, false, zero
/// or empty says nothing.
pub(crate) fn said_something(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(held) => *held,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(members) => !members.is_empty(),
    }
}

/// The exit of a test's provider, whose key is the placeholder every test
/// configures.
#[cfg(test)]
pub(crate) fn test_exit() -> roomkit_providers_client::Exit {
    roomkit_providers_client::harness::exit("anthropic", ["placeholder-not-a-key".to_owned()])
}

#[cfg(test)]
mod tests {
    use roomkit_ai::ProviderError;
    use serde_json::json;

    use super::*;

    /// The failure of a status, as it leaves a provider whose key is the
    /// placeholder.
    fn refusal_of(code: u16, body: &[u8]) -> ProviderError {
        test_exit().leave(&status(code, body))
    }

    // What a host sees of a refused request: the error object's own words,
    // and nothing of a body that is no JSON or of another shape.
    #[test]
    fn a_status_failure_says_what_the_error_object_says_and_echoes_no_other_body() {
        let said = |status: u16, body: &str| refusal_of(status, body.as_bytes()).message;
        let object = json!({
            "type": "error",
            "error": {"type": "authentication_error", "message": "invalid x-api-key"},
            "request_id": "req_1",
        });
        assert_eq!(said(401, &object.to_string()), "invalid x-api-key");
        let page = "<html><body><h1>502 Bad Gateway</h1>nginx at 10.0.0.7</body></html>";
        assert_eq!(
            said(502, page),
            "the provider answered HTTP 502: Bad Gateway"
        );
        assert_eq!(
            said(400, r#"{"detail": "model not loaded"}"#),
            "the provider answered HTTP 400: Bad Request"
        );
        assert_eq!(said(529, ""), "the provider answered HTTP 529: no message");
        // A byte order mark before the body is no part of it.
        let marked = format!("\u{feff}{object}");
        assert_eq!(said(401, &marked), "invalid x-api-key");
    }

    // Source: src/tests.rs::stream_errors_retain_retryability_and_redact_the_active_key
    #[test]
    fn a_failure_never_carries_the_key_and_a_status_decides_whether_it_is_retried() {
        let echoed = json!({"error": {
            "type": "placeholder-not-a-key",
            "message": "Wrong key: placeholder-not-a-key.",
        }});
        let error = refusal_of(401, echoed.to_string().as_bytes());
        assert_eq!(error.message, "Wrong key: [***].");
        assert!(!format!("{error} {error:?}").contains("placeholder-not-a-key"));
        for (status, retryable) in [(400, false), (401, false), (408, true), (409, true)] {
            assert_eq!(refusal_of(status, b"{}").retryable, retryable, "{status}");
        }
        assert!(refusal_of(429, b"").retryable && refusal_of(529, b"").retryable);
    }

    // Anthropic names no code for a context too long: its words are read,
    // in the whole body.
    #[test]
    fn a_context_too_long_is_read_from_the_words_of_the_body() {
        let too_long = json!({"error": {
            "type": "invalid_request_error",
            "message": "prompt is too long: 210000 tokens > 200000 maximum",
        }});
        let error = refusal_of(400, too_long.to_string().as_bytes());
        assert_eq!(error.context_overflow, Some(true));
        assert_eq!(refusal_of(400, b"{}").context_overflow, None);
    }

    #[test]
    fn an_answer_with_no_event_fails_as_the_error_it_holds_or_as_no_event() {
        let overloaded = json!({
            "type": "error",
            "error": {"type": "overloaded_error", "message": "Overloaded"},
        });
        let error = test_exit().leave(&no_event(Some(&overloaded)));
        assert_eq!(
            (error.message.as_str(), error.status_code, error.retryable),
            ("Overloaded", Some(529), true)
        );
        for none in [
            None,
            Some(json!({"status": "ok"})),
            Some(json!({"error": null})),
        ] {
            let error = test_exit().leave(&no_event(none.as_ref()));
            assert_eq!(
                error.message,
                "The provider's stream carried no event: its body could not be read"
            );
            assert_eq!((error.status_code, error.retryable), (None, false));
        }
    }
}
