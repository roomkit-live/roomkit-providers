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

mod public_url;
mod trust;

pub use public_url::{ValidatePublicUrlOptions, validate_public_url};
pub use trust::{CertificateRefused, TlsUnset, certificate_refused};

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

/// TLS with ring and HTTP/1.1 ALPN, for a provider crate: the configuration
/// its HTTP client is built with.
///
/// A server's certificate is checked against what the environment and the
/// system name at the moment of the call, as the client of `RoomKit`'s
/// Python providers decides it: the authorities of the file `SSL_CERT_FILE`
/// names, and no other; else those filed in the directories `SSL_CERT_DIR`
/// names, under the names OpenSSL gives them; else the system's own
/// verifier on macOS and Windows, and elsewhere the store of authorities
/// the system keeps on its disk. A system that keeps none is given the
/// public roots compiled in, which are never added to a store that holds an
/// authority.
///
/// # Errors
///
/// The cryptography provider has no usable protocol version; the file
/// `SSL_CERT_FILE` names does not read or holds no certificate; or the
/// system's verifier could not be made.
#[doc(hidden)]
pub fn tls_config() -> Result<rustls::ClientConfig, TlsUnset> {
    trust::tls_config()
}
