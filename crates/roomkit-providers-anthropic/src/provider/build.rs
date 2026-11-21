//! What a provider is built from: what every request carries, which key a
//! request goes with, whether the provider talks to Anthropic's own
//! endpoint, and where its failures leave.

use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use roomkit_providers_client::endpoint::talks_to_vendor;
use roomkit_providers_client::{Client, Exit, Failure, configured_header};

use super::{JSON, LABEL, MESSAGES};
use crate::config::AnthropicConfig;
use crate::endpoint::ANTHROPIC_BASE_URL;

/// The header a request carries its key in.
pub(super) const KEY_HEADER: HeaderName = HeaderName::from_static("x-api-key");

/// The header that names the version of the API a request speaks.
const VERSION_HEADER: HeaderName = HeaderName::from_static("anthropic-version");

/// The version of the Messages API the provider speaks.
const VERSION: &str = "2023-06-01";

/// Where the failures of the provider of `config` leave, made by its
/// `client`. They repeat nothing of the key, of the value of each extra
/// header and of its credentials when it names a scheme before them, of the
/// user information of the base URL, as written and as it reads, nor of
/// what the client alone knows: the `Authorization` it writes from that
/// user information, and the credentials of a proxy.
pub(super) fn exit_of(config: &AnthropicConfig, client: &Client, url: &reqwest::Url) -> Exit {
    let headers = config.extra_headers.iter().flatten();
    let headers = headers.map(|(_, value)| value.as_str());
    client.exit(LABEL, [config.api_key.clone()], headers, url)
}

/// Whether the provider talks to Anthropic's own endpoint, whose models
/// and rules it knows: the base URL is Anthropic's as Python compares it,
/// and the URL the client dials is Anthropic's as the client reads it.
pub(super) fn own_endpoint(config: &AnthropicConfig, dialed: &reqwest::Url) -> bool {
    let base_url = config.kept_base_url();
    talks_to_vendor(base_url.as_deref(), &[ANTHROPIC_BASE_URL], dialed, MESSAGES)
}

/// `key` as a header's value, which no `Debug` form shows; `None` for a key
/// that cannot go in a header.
pub(super) fn key_value(key: &str) -> Option<HeaderValue> {
    let mut value = HeaderValue::from_str(key).ok()?;
    value.set_sensitive(true);
    Some(value)
}

/// The headers every request carries, and what they say of its key.
pub(super) struct Carried {
    /// What the provider writes, then the configuration's extra headers.
    pub(super) headers: HeaderMap,
    /// Whether an extra header carries the key, in place of the
    /// configuration's and of one a request brings.
    pub(super) key_replaced: bool,
    /// Whether a request that brings no key still carries a credential:
    /// the key its headers hold, or an `Authorization` among the extra
    /// headers.
    pub(super) credential: bool,
}

/// The headers every request carries: the key, the version of the API and
/// what the provider accepts, then the configuration's extra headers, one
/// of which replaces the provider's of the same name, its key included, as
/// Python's client lets it, its value shown by no `Debug` form. An empty
/// key is written as an empty header, as Python's SDK writes it. A request
/// goes with it only beside a credential an extra header brings: a
/// request's own key takes its place, and a request with no credential is
/// not sent.
///
/// A base URL that holds user information is sent with it as Basic
/// credentials, which the client writes beside the key, in place of a
/// configured `Authorization`, as Python's client does.
pub(super) fn headers_of(config: &AnthropicConfig) -> Result<Carried, Failure> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static(JSON));
    headers.insert(VERSION_HEADER, HeaderValue::from_static(VERSION));
    let no_header = || Failure::unbuilt("its key cannot go in a header");
    let key = key_value(&config.api_key).ok_or_else(no_header)?;
    headers.insert(KEY_HEADER, key);
    let (mut key_replaced, mut authorized) = (false, false);
    for (name, value) in config.extra_headers.iter().flatten() {
        let no_header = || Failure::unbuilt("one of its extra headers is no HTTP header");
        let (name, value) = configured_header(name, value).ok_or_else(no_header)?;
        authorized |= name == AUTHORIZATION && !value.is_empty();
        key_replaced |= name == KEY_HEADER;
        headers.insert(name, value);
    }
    let keyed = headers.get(KEY_HEADER).is_some_and(|key| !key.is_empty());
    Ok(Carried {
        headers,
        key_replaced,
        credential: keyed || authorized,
    })
}
