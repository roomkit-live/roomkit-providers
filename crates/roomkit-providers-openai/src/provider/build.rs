//! What a provider is built from: what every request carries, whether it
//! talks to `OpenAI`'s own endpoint, and what no failure may repeat of its
//! configuration.

use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use roomkit_providers_client::endpoint::{has_user_information, talks_to_vendor};
use roomkit_providers_client::{Client, Failure, Secrets};

use super::{CHAT_COMPLETIONS, JSON};
use crate::config::OpenAiConfig;
use crate::endpoint::OPENAI_BASE_URL;

/// What no failure may repeat of `config`: the key, the value of each
/// default header and its credentials when it names a scheme before them,
/// the user information of the base URL, as written and as it reads, and
/// the `Authorization` the `client` writes from that user information.
pub(super) fn secrets_of(config: &OpenAiConfig, client: &Client, chat: &reqwest::Url) -> Secrets {
    let headers = config.default_headers.iter().flatten();
    let headers = headers.map(|(_, value)| value.as_str());
    client.secrets([config.api_key.clone()], headers, chat)
}

/// Whether the provider talks to `OpenAI`'s own endpoint, whose models and
/// rules it knows: the base URL is `OpenAI`'s as Python compares it, and
/// the URL the client dials is `OpenAI`'s as the client reads it.
pub(super) fn own_endpoint(config: &OpenAiConfig, dialed: &reqwest::Url) -> bool {
    let base_url = config.base_url.as_deref();
    talks_to_vendor(base_url, &[OPENAI_BASE_URL], dialed, CHAT_COMPLETIONS)
}

/// The headers every request carries: what the provider writes, then the
/// configuration's, one of which replaces the provider's of the same name
/// as Python's client lets it. An empty key sends no `Authorization`. A
/// request with a body says beside them what the body is.
///
/// A base URL that holds user information is sent with it as Basic
/// credentials, which the client writes: as with Python's client, they
/// take the place of the key and of a configured `Authorization`, so
/// neither is written here.
pub(super) fn headers_of(config: &OpenAiConfig, chat: &reqwest::Url) -> Result<HeaderMap, Failure> {
    let basic = has_user_information(chat);
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static(JSON));
    if !config.api_key.is_empty() && !basic {
        let bearer = HeaderValue::from_str(&format!("Bearer {}", config.api_key));
        let mut bearer = bearer.map_err(|_| Failure::unbuilt("its key cannot go in a header"))?;
        bearer.set_sensitive(true);
        headers.insert(AUTHORIZATION, bearer);
    }
    for (name, value) in config.default_headers.iter().flatten() {
        let no_header = || Failure::unbuilt("one of its default headers is no HTTP header");
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| no_header())?;
        let mut value = HeaderValue::from_str(value).map_err(|_| no_header())?;
        if basic && name == AUTHORIZATION {
            continue;
        }
        value.set_sensitive(name == AUTHORIZATION);
        headers.insert(name, value);
    }
    Ok(headers)
}
