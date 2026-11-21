//! `OpenAI`'s own endpoint, and whether a base URL is it.

use roomkit_providers_client::endpoint::is_vendor_endpoint;
pub(crate) use roomkit_providers_client::endpoint::shown_url;

/// `OpenAI`'s own endpoint, which a configuration with no base URL calls.
pub(crate) const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

/// Whether `base_url` is `OpenAI`'s own endpoint: none given, or
/// [`OPENAI_BASE_URL`] however it is written (Python's
/// `is_vendor_endpoint(base_url, OPENAI_BASE_URL)`).
pub(crate) fn is_openai_endpoint(base_url: Option<&str>) -> bool {
    is_vendor_endpoint(base_url, &[OPENAI_BASE_URL])
}
