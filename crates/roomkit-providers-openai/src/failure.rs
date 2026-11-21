//! What a failed request reads as on `OpenAI`'s wire: how the body of a
//! refused request is read, and the failures of a completion in the
//! provider's own words.
//!
//! A failure is a [`Failure`] of `roomkit-providers-client`, which keeps
//! the provider's own words apart from what a server, a reader or the
//! contract said, and words the failures every provider shares. It leaves
//! the provider through that crate's `Exit`, the one place the contract's
//! error is made: there what was said is cleaned of the configuration's
//! secrets, put on one line and bounded.

pub(crate) use roomkit_providers_client::failure::{Failure, MESSAGE_LIMIT, past_mark};
use roomkit_providers_client::failure::{Stated, one_line, own, told};
use serde_json::Value;

#[cfg(test)]
mod exit_tests;

/// What Python's client says of an error inside a stream that brings no
/// message of its own.
const STREAM_ERROR: &str = "An error occurred during streaming";

/// The failure of an answer whose status is neither a success nor a
/// redirect: `the provider answered HTTP 401 (type, code): message`. Of
/// the body it carries what an error object says of itself and nothing
/// else: its message, its type and its code, read under `error` or at the
/// top level, and, where it states no message, the text it words its
/// refusal with under `detail`. A body that is no JSON is not echoed: the
/// text is then the status's reason. Whether the request overflowed the
/// model's context is read from the whole body, echoed or not: the code
/// that says so, or the words.
pub(crate) fn status(status: u16, body: &[u8]) -> Failure {
    let parsed: Option<Value> = serde_json::from_slice(past_mark(body)).ok();
    let stated = |name: &str| {
        let stated = said_of(parsed.as_ref()?, name)?;
        Some(stated).filter(|stated| !one_line(stated).is_empty())
    };
    let (kind, code) = (stated("type"), stated("code"));
    // The code is compared as the server wrote it.
    let overflow = code.as_deref() == Some("context_length_exceeded");
    let worded = |name: &str| {
        let worded = parsed.as_ref()?.get(name)?.as_str()?;
        Some(worded.to_owned()).filter(|worded| !one_line(worded).is_empty())
    };
    let bare = || parsed.as_ref()?.get("error")?.as_str().map(str::to_owned);
    let message = stated("message").or_else(bare);
    let stated = Stated {
        kind,
        code,
        message: message.or_else(|| worded("detail")),
        overflow,
    };
    Failure::refused(status, body, stated)
}

/// The failure of an error a server wrote inside its stream, in place of
/// an event: the status the error names decides whether it is retried,
/// else its words (Python's `provider_error`). Its text is the error's own
/// message, or the words of Python's client for an error that brings none.
pub(crate) fn stream_error(error: &Value) -> Failure {
    Failure::stream_error(error, STREAM_ERROR)
}

/// The failure of a streamed answer that ended with neither a finish
/// reason nor its end mark: it was cut, as one whose connection is lost,
/// and is worth a retry.
pub(crate) fn cut_short() -> Failure {
    let said = "the provider's stream ended with neither a finish reason nor its end mark";
    Failure::said(vec![own(said)], true, None)
}

/// The failure of an answer of status 200 that is no chat completion:
/// final. Where its `body` is JSON and words why, under `message` or else
/// under `detail`, as a text, the failure carries those words.
pub(crate) fn no_completion(body: Option<&Value>) -> Failure {
    let worded = |name: &str| {
        let worded = body?.get(name)?.as_str()?;
        Some(worded.to_owned()).filter(|worded| !one_line(worded).is_empty())
    };
    let said = "The provider answered with a body it could not read: not a chat completion";
    let mut said = vec![own(said)];
    if let Some(words) = worded("message").or_else(|| worded("detail")) {
        said.extend([own(": "), told(words, MESSAGE_LIMIT)]);
    }
    Failure::said(said, false, None)
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

/// The exit of a test's provider, whose key is the placeholder every test
/// configures.
#[cfg(test)]
pub(crate) fn test_exit() -> roomkit_providers_client::Exit {
    let secrets = roomkit_providers_client::Secrets::of(["placeholder-not-a-key".to_owned()]);
    roomkit_providers_client::Exit::new("openai", secrets)
}

#[cfg(test)]
mod tests {
    use roomkit_providers_client::failure::NAME_LIMIT;
    use roomkit_providers_llm::ProviderError;
    use serde_json::json;

    use super::*;

    /// The failure of a status, as it leaves a provider whose key is the
    /// placeholder.
    fn refusal_of(code: u16, body: &[u8]) -> ProviderError {
        test_exit().leave(&status(code, body))
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
}
