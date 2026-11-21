//! Outbound URL validation for applications, and the HTTP support the model
//! providers of this repository share.
//!
//! - [`validate_public_url`], with [`ValidatePublicUrlOptions`]: checks that
//!   a URL names a public destination before a transport is given it, as
//!   `RoomKit`'s Python `validate_public_url` does. Its documentation says
//!   what it does not guard: a name the client resolves again, a client
//!   behind a proxy, redirects, and a name when resolution is turned off.
//!
//! The crate also holds what the provider crates build their HTTP on, hidden
//! from this documentation: a bounded reader of server-sent events and the
//! TLS configuration their clients take. It depends on neither Python nor
//! `RoomKit`, and on no HTTP client: it reads a host with the `url` crate,
//! the parser `reqwest` reads a URL with.

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
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| error.to_string())?
    .with_root_certificates(public_roots())
    .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

/// The public web PKI roots a server's certificate is checked against.
fn public_roots() -> rustls::RootCertStore {
    rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use rustls::pki_types::ServerName;

    use super::*;

    /// The `supported_versions` extension of a hello that offers TLS 1.3,
    /// then TLS 1.2: its type, its length, then the two versions.
    const TLS_1_3_THEN_1_2: [u8; 9] = [0x00, 0x2b, 0x00, 0x05, 0x04, 0x03, 0x04, 0x03, 0x03];

    #[test]
    fn the_tls_configuration_is_built_and_asks_for_http_1_1() {
        let config = tls_config().unwrap();
        assert_eq!(config.alpn_protocols, [b"http/1.1".to_vec()]);
    }

    /// The versions are read where a server reads them: in the hello a
    /// client of the configuration writes first, which no socket carries
    /// here.
    #[test]
    fn a_client_of_the_tls_configuration_offers_tls_1_3_and_tls_1_2() {
        let config = Arc::new(tls_config().unwrap());
        let server = ServerName::try_from("service.example").unwrap();
        let mut client = rustls::ClientConnection::new(config, server).unwrap();
        let mut hello = Vec::new();
        client.write_tls(&mut hello).unwrap();
        let offered = hello
            .windows(TLS_1_3_THEN_1_2.len())
            .any(|bytes| bytes == TLS_1_3_THEN_1_2);
        assert!(offered, "{hello:02x?}");
    }

    #[test]
    fn the_root_store_holds_the_public_web_pki_roots() {
        let roots = public_roots();
        assert!(!roots.is_empty());
        assert_eq!(roots.len(), webpki_roots::TLS_SERVER_ROOTS.len());
    }
}
