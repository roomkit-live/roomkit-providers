//! What a provider is built from: what every request carries, whether it
//! talks to its vendor's own endpoint, and where its failures leave.

use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue};
use roomkit_providers_client::endpoint::{has_user_information, talks_to_vendor};
use roomkit_providers_client::{Client, Exit, Failure, configured_header};

use super::JSON;
use crate::config::OpenAiConfig;
use crate::vendor::Vendor;

/// Where the failures of `vendor`'s provider of `config` leave, made by its
/// `client`, under the name Python's `_provider_name` gives it. They repeat
/// nothing of the key, of the value of each default header and of its
/// credentials when it names a scheme before them, of the user information
/// of the base URL, as written and as it reads, nor of what the client
/// alone knows: the `Authorization` it writes from that user information,
/// and the credentials of a proxy.
pub(super) fn exit_of(
    config: &OpenAiConfig,
    vendor: Vendor,
    client: &Client,
    chat: &reqwest::Url,
) -> Exit {
    let headers = config.default_headers.iter().flatten();
    let headers = headers.map(|(_, value)| value.as_str());
    let name = vendor.provider();
    client.exit(name, [config.api_key.clone()], headers, chat)
}

/// Whether the provider talks to `vendor`'s own endpoint, whose models and
/// rules it knows: the base URL is the vendor's as Python compares it, and
/// the URL the client dials is the vendor's as the client reads it.
pub(super) fn own_endpoint(config: &OpenAiConfig, vendor: Vendor, dialed: &reqwest::Url) -> bool {
    let base_url = config.base_url.as_deref();
    talks_to_vendor(base_url, vendor.base_urls(), dialed, config.api.path())
}

/// The headers every request carries: what the provider writes, then the
/// configuration's, one of which replaces the provider's of the same name
/// as Python's client lets it, its value shown by no `Debug` form. An empty
/// key sends no `Authorization`. A request with a body says beside them
/// what the body is.
///
/// A base URL that holds user information is sent with it as Basic
/// credentials, which the client writes in place of any `Authorization`
/// here, the key's or a configured one, as Python's client does. The key
/// is then not written at all: one that can go in no header still builds
/// a provider that never sends it.
pub(super) fn headers_of(config: &OpenAiConfig, chat: &reqwest::Url) -> Result<HeaderMap, Failure> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static(JSON));
    if !config.api_key.is_empty() && !has_user_information(chat) {
        let bearer = HeaderValue::from_str(&format!("Bearer {}", config.api_key));
        let mut bearer = bearer.map_err(|_| Failure::unbuilt("its key cannot go in a header"))?;
        bearer.set_sensitive(true);
        headers.insert(AUTHORIZATION, bearer);
    }
    for (name, value) in config.default_headers.iter().flatten() {
        let no_header = || Failure::unbuilt("one of its default headers is no HTTP header");
        let (name, value) = configured_header(name, value).ok_or_else(no_header)?;
        headers.insert(name, value);
    }
    Ok(headers)
}
