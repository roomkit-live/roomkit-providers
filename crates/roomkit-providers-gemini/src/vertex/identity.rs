//! Who a provider on Vertex AI calls as, and the token each of its requests
//! carries: `roomkit-auth`'s `GoogleCloud`, as Python's provider builds its
//! credentials (`GeminiVertexProvider._credentials`).
//!
//! The identity is the configuration's service-account key, else
//! Application Default Credentials found as google-auth finds them; a
//! borrowed service account is borrowed with it. A key and an
//! Application Default Credentials file are read when the provider is
//! built; a token is asked at the first request and kept as google-auth
//! keeps it. A token that cannot be had fails the request with the
//! client's closed words: it says neither the endpoint nor what it
//! answered.

use roomkit_auth::{Environment, GoogleCloud};
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

/// The identity `config` names, Application Default Credentials read in
/// `environment`.
///
/// # Errors
///
/// A key that is no service-account key.
pub(crate) fn of(
    config: &GeminiVertexConfig,
    environment: &Environment,
) -> Result<GoogleCloud, Failure> {
    let given = |text: &Option<String>| {
        let text = text.as_deref().map(str::trim);
        text.filter(|text| !text.is_empty()).map(str::to_owned)
    };
    let unbuilt = |_| Failure::unbuilt("its identity cannot be built");
    let identity = match given(&config.service_account_json) {
        Some(key) => GoogleCloud::service_account(&key, USER_AGENT)
            .map_err(|_| Failure::unbuilt("its service-account key is no key"))?,
        None => GoogleCloud::application_default_in(environment, USER_AGENT).map_err(unbuilt)?,
    };
    match given(&config.impersonate_service_account) {
        Some(target) => identity.borrowing(&target).map_err(unbuilt),
        None => Ok(identity),
    }
}
