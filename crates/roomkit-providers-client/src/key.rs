//! The key one request brings for itself.

use roomkit_ai::{API_KEY_METADATA_KEY, AiContext};
use serde_json::Value;

/// The key `context` lends its generation, in place of the configuration's:
/// the text its metadata holds under
/// [`API_KEY_METADATA_KEY`](roomkit_ai::API_KEY_METADATA_KEY), when it is
/// not empty.
#[must_use]
pub fn request_api_key(context: &AiContext) -> Option<&str> {
    context
        .metadata
        .get(API_KEY_METADATA_KEY)
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty())
}
