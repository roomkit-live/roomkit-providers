//! Anthropic's own endpoint, and whether a base URL is it.

use roomkit_providers_client::endpoint::is_vendor_endpoint;
pub(crate) use roomkit_providers_client::endpoint::shown_url;

/// Anthropic's own endpoint, which a configuration with no base URL calls.
pub(crate) const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";

/// Whether `base_url` is Anthropic's own endpoint: none given, or
/// [`ANTHROPIC_BASE_URL`] however it is written (Python's
/// `is_vendor_endpoint(base_url, ANTHROPIC_BASE_URL)`).
pub(crate) fn is_anthropic_endpoint(base_url: Option<&str>) -> bool {
    is_vendor_endpoint(base_url, &[ANTHROPIC_BASE_URL])
}
