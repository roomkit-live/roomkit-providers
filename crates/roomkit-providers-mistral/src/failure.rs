//! What a failed request reads as on Mistral's API: how the body of a
//! refused request is read, and the failures of an answer in the
//! provider's own words (Python's `provider_error`, over the mistralai
//! SDK's errors).
//!
//! A failure is a [`Failure`] of `roomkit-providers-client`, which keeps
//! the provider's own words apart from what a server or a reader said. It
//! leaves the provider through that crate's `Exit`, where what was said is
//! cleaned of the configuration's secrets, put on one line and bounded.

pub(crate) use roomkit_providers_client::failure::{Failure, past_mark};
use roomkit_providers_client::failure::{MESSAGE_LIMIT, one_line, own, retryable_status, told};
use serde_json::Value;

/// A refused request: its status, retryable as the status is (429, 5xx),
/// and what the body says. Python reads no overflow of the model's context
/// from Mistral's errors: none is stated.
pub(crate) fn status(status: u16, body: &[u8]) -> Failure {
    let mut said = vec![own(format!("the provider answered HTTP {status}"))];
    if let Some(message) = message(body) {
        said.extend([own(": "), told(message, MESSAGE_LIMIT)]);
    }
    Failure::said(said, retryable_status(status), Some(status))
}

/// What the body of a refused request says: Mistral's `message`, or the
/// body itself.
fn message(body: &[u8]) -> Option<String> {
    let parsed: Option<Value> = serde_json::from_slice(past_mark(body)).ok();
    let stated = parsed.as_ref().and_then(|parsed| parsed.get("message"));
    let stated = stated.and_then(Value::as_str).map(str::to_owned);
    let text = stated.unwrap_or_else(|| String::from_utf8_lossy(body).into_owned());
    Some(text).filter(|text| !one_line(text).is_empty())
}

/// A 2xx answer that says it is no stream of events, which the SDK reads
/// no stream from.
pub(crate) fn no_stream() -> Failure {
    Failure::unreadable("the answer is no stream of events")
}

/// An event whose data is not a chunk as the SDK reads one.
pub(crate) fn no_chunk(error: &serde_json::Error) -> Failure {
    Failure::malformed(error)
}

/// A listing that is no page of models.
pub(crate) fn no_listing() -> Failure {
    Failure::unreadable("the model listing is no page of models")
}
