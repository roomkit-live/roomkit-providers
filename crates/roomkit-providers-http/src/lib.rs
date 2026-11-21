//! Outbound URL validation for applications, and the HTTP support the model
//! providers of this repository share.
//!
//! - [`validate_public_url`], with [`ValidatePublicUrlOptions`]: checks that
//!   a URL names a public destination before a transport is given it, as
//!   `RoomKit`'s Python `validate_public_url` does.
//!
//! The crate also holds what the provider crates build their HTTP on, hidden
//! from this documentation: a bounded reader of server-sent events and the
//! TLS configuration their clients take. It depends on neither Python nor
//! `RoomKit`.

#![forbid(unsafe_code)]

use std::sync::Arc;

mod public_url;

pub use public_url::{ValidatePublicUrlOptions, validate_public_url};

#[doc(hidden)]
pub mod sse;

/// A bounded event stream that could not be decoded.
///
/// For a provider crate, with [`sse::Events`].
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SseError {
    /// The stream exceeded its byte budget.
    #[error("the provider's answer is longer than {limit} bytes")]
    AnswerTooLong {
        /// The maximum number of bytes.
        limit: usize,
    },
    /// A line exceeded its byte budget.
    #[error("the provider's stream sent a line longer than {limit} bytes")]
    StreamLineTooLong {
        /// The maximum number of bytes.
        limit: usize,
    },
    /// An event exceeded its byte budget.
    #[error("the provider's stream sent an event longer than {limit} bytes")]
    StreamEventTooLong {
        /// The maximum number of bytes.
        limit: usize,
    },
    /// The wire was not valid UTF-8.
    #[error("the provider's answer is malformed: {reason}")]
    Malformed {
        /// The reason, without the response's content.
        reason: String,
    },
}

/// TLS with ring, the public web PKI roots, and HTTP/1.1 ALPN.
///
/// For a provider crate: the configuration its HTTP client is built with.
///
/// # Errors
///
/// Fails when the cryptography provider has no usable protocol version.
#[doc(hidden)]
pub fn tls_config() -> Result<rustls::ClientConfig, String> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| error.to_string())?
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tls_configuration_is_built_and_asks_for_http_1_1() {
        let config = tls_config().unwrap();
        assert_eq!(config.alpn_protocols, [b"http/1.1".to_vec()]);
    }
}
