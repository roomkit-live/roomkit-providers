//! How a failure reads: the status an error names, whether it is retried,
//! and what its text may hold.

use roomkit_providers_llm::{AiProvider, AiRequest};
use serde_json::{Value, json};

use super::support::{KEY, elsewhere, hello, pointed, start, stream_of, streamed};
use crate::replay::fake::{Fake, Reply};

/// A stream that starts a message, then tells the error `body` holds.
fn telling(body: &Value) -> Reply {
    let mut error = body.clone();
    error["type"] = "error".into();
    stream_of(&[start(), error])
}

// Source: src/tests/wire.rs::nested_error_codes_and_types_preserve_retryability
// Python: src/roomkit/providers/ai/base.py::provider_error, the status an
// error body describes
#[tokio::test]
async fn nested_error_codes_and_types_preserve_retryability() {
    for (body, status) in [
        (
            json!({"error": {"code": 429, "message": "capacity exhausted"}}),
            429,
        ),
        (json!({"error": {"type": "service_unavailable"}}), 503),
        (
            json!({"code": 400, "error": {"data": {"status": "DEADLINE_EXCEEDED"}}}),
            504,
        ),
        (json!({"error": {"code": "401"}}), 401),
    ] {
        let (_, failure) = streamed(telling(&body), &hello()).await;
        let error = failure.unwrap();
        assert_eq!(error.status_code, Some(status), "{body}");
        assert_eq!(error.retryable, status != 401, "{body}");
        // The status the server answered decides, not the one its body
        // names.
        let (_, failure) = streamed(Reply::json(403, body.to_string()), &hello()).await;
        let error = failure.unwrap();
        assert_eq!((error.status_code, error.retryable), (Some(403), false));
    }
    for invalid in [json!(200), json!(true), json!(999), json!("200")] {
        let body = json!({"error": {"code": invalid, "message": "The server is overloaded."}});
        let (_, failure) = streamed(telling(&body), &hello()).await;
        let error = failure.unwrap();
        // An error that names no status is final, whatever its words say.
        assert_eq!(
            (error.status_code, error.retryable),
            (None, false),
            "{body}"
        );
    }
}

// Source: src/tests.rs::stream_errors_retain_retryability_and_redact_the_active_key
// Python: tests/test_providers/test_anthropic.py::test_sdk_error_wrapped_in_provider_error
#[tokio::test]
async fn stream_errors_retain_retryability_and_hide_the_key_of_the_request_too() {
    const LENT: &str = "placeholder-lent-key";
    let said = format!("{KEY} and {LENT} overloaded");
    let overloaded = json!({"error": {"type": "overloaded_error", "message": said}});
    let mut lending = hello();
    lending.metadata.insert("api_key".to_owned(), LENT.into());
    for request in [hello(), lending] {
        let lent = request.metadata.contains_key("api_key");
        let (events, failure) = streamed(telling(&overloaded), &request).await;
        assert_eq!(events.len(), 1, "what the start told: {events:?}");
        let error = failure.unwrap();
        assert_eq!((error.status_code, error.retryable), (Some(529), true));
        assert_eq!(error.provider, "anthropic");
        let shown = format!("{error} {error:?}");
        assert!(!shown.contains(KEY), "{shown}");
        // The key a request lends is hidden in that request's failures.
        assert_eq!(shown.contains(LENT), !lent, "{shown}");
    }
    // The whole answer fails the same way, and the listing hides the
    // configuration's key.
    for status in [401, 529] {
        let body = json!({"error": {"type": "authentication_error", "message": said}});
        let reply = || Reply::json(status, body.to_string());
        let fake = Fake::serving(vec![reply(), reply()]).await;
        let provider = pointed(elsewhere("local-model"), &fake);
        let request = AiRequest {
            metadata: [("api_key".to_owned(), Value::from(LENT))]
                .into_iter()
                .collect(),
            ..hello()
        };
        let whole = provider.generate(&request).await.unwrap_err();
        let listing = provider.list_models().await.unwrap_err();
        for error in [&whole, &listing] {
            assert_eq!(error.status_code, Some(status));
            assert_eq!(error.retryable, status == 529);
            assert!(!format!("{error} {error:?}").contains(KEY), "{error:?}");
        }
        assert!(!format!("{whole}").contains(LENT), "{whole}");
    }
}
