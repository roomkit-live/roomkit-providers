//! What a failed request reads as on the Developer API: how the body of a
//! refused request is read, and the failures of a stream in the provider's
//! own words (Python's `wrap_gemini_error`, through `provider_error`).
//!
//! A failure is a [`Failure`] of `roomkit-providers-client`, which keeps
//! the provider's own words apart from what a server or a reader said. It
//! leaves the provider through that crate's `Exit`, where what was said is
//! cleaned of the configuration's secrets, put on one line and bounded.

use roomkit_ai::is_context_overflow_message;
pub(crate) use roomkit_providers_client::failure::{Failure, past_mark};
use roomkit_providers_client::failure::{MESSAGE_LIMIT, Stated, one_line, retryable_status, told};
use serde_json::Value;

/// A refused request: what Google's error object says (`error.message`, its
/// `status` as its kind), retryable as its status is (429, 5xx).
pub(crate) fn status(status: u16, body: &[u8]) -> Failure {
    let parsed: Option<Value> = serde_json::from_slice(past_mark(body)).ok();
    let stated = |name: &str| {
        let stated = parsed.as_ref()?.get("error")?.get(name)?.as_str()?;
        Some(stated.to_owned()).filter(|stated| !one_line(stated).is_empty())
    };
    let Some(message) = stated("message") else {
        let stated = Stated {
            kind: stated("status"),
            code: None,
            message: None,
            overflow: false,
        };
        return Failure::refused(status, body, stated);
    };
    let said = vec![told(message, MESSAGE_LIMIT)];
    let failure = Failure::said(said, retryable_status(status), Some(status));
    if is_context_overflow_message(&String::from_utf8_lossy(body)) {
        failure.overflowing()
    } else {
        failure
    }
}

/// A stream that carried no event: its body could not be read, or it is a
/// JSON error object in place of a stream.
pub(crate) fn no_event(answer: Option<&Value>) -> Failure {
    let error = answer.filter(|answer| answer.get("error").is_some_and(|error| !error.is_null()));
    error.map_or_else(Failure::no_event, Failure::error_object)
}

/// A listing that is no page of models.
pub(crate) fn no_listing() -> Failure {
    Failure::unreadable("the model listing is no page of models")
}
