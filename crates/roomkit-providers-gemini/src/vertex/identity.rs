//! Who a provider on Vertex AI calls as, and the token each of its requests
//! carries: `roomkit-auth`'s `GoogleCloud`, as Python's provider builds its
//! credentials (`GeminiVertexProvider._credentials`).
//!
//! A service-account key is read when the provider is built; its token is
//! asked at the first request and kept as google-auth keeps it. A token
//! that cannot be had fails the request with the client's closed words: it
//! says neither the endpoint nor what it answered.

use roomkit_auth::GoogleCloud;
use roomkit_providers_client::{TokenError, TokenFuture, TokenSource, USER_AGENT};

use super::config::GeminiVertexConfig;
use crate::failure::Failure;

/// The identity of a provider, as the client asks its tokens.
pub(crate) struct Identity(pub(crate) GoogleCloud);

impl TokenSource for Identity {
    fn token(&self) -> TokenFuture<'_> {
        Box::pin(async move {
            let token = self.0.access_token().await;
            let token = token.map_err(|_| TokenError::Unavailable)?;
            Ok(token.expose_secret().to_owned())
        })
    }
}

/// The identity `config` names: its service-account key, read now.
///
/// # Errors
///
/// A key that is no service-account key; an identity this version does not
/// take yet: Application Default Credentials (no key) and a borrowed
/// service account.
pub(crate) fn of(config: &GeminiVertexConfig) -> Result<GoogleCloud, Failure> {
    let given = |text: &Option<String>| {
        let text = text.as_deref().map(str::trim);
        text.filter(|text| !text.is_empty()).map(str::to_owned)
    };
    let key = given(&config.service_account_json);
    let borrowed = given(&config.impersonate_service_account);
    match (key, borrowed) {
        (Some(key), None) => GoogleCloud::service_account(&key, USER_AGENT)
            .map_err(|_| Failure::unbuilt("its service-account key is no key")),
        (None, _) => Err(Failure::unbuilt(
            "it names no service-account key: Application Default Credentials are not taken yet",
        )),
        (Some(_), Some(_)) => Err(Failure::unbuilt(
            "a borrowed service account is not taken yet",
        )),
    }
}
