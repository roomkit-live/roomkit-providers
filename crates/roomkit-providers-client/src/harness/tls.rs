//! An authority made for a test, and the TLS of the fakes it signs for.
//!
//! Its key is made when the test runs and kept in the test's memory alone:
//! no key is written to the repository, and none outlives the test.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, DnType, IsCa, KeyPair};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

/// How many authorities this process made: each is named by its number, so
/// that a client tells one from another by name, as it tells two real
/// authorities apart.
static MADE: AtomicU32 = AtomicU32::new(0);

/// A certificate authority of a test's own.
#[derive(Debug)]
pub struct Authority(CertifiedIssuer<'static, KeyPair>);

impl Default for Authority {
    fn default() -> Self {
        Self::new()
    }
}

impl Authority {
    /// A new authority, which no client trusts unless it is told to.
    #[must_use]
    pub fn new() -> Self {
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let number = MADE.fetch_add(1, Ordering::Relaxed);
        let name = format!("authority {number} of a test");
        params.distinguished_name.push(DnType::CommonName, name);
        let key = KeyPair::generate().unwrap();
        Self(CertifiedIssuer::self_signed(params, key).unwrap())
    }

    /// The authority's certificate, in PEM: what a file `SSL_CERT_FILE`
    /// names holds for a client to trust what the authority signs.
    #[must_use]
    pub fn pem(&self) -> String {
        self.0.pem()
    }

    /// The authority's certificate, for a store a test fills itself.
    #[must_use]
    pub fn certificate(&self) -> rustls::pki_types::CertificateDer<'static> {
        self.0.der().clone()
    }

    /// The TLS of a server whose certificate this authority signed for
    /// `names`, each a host name or an address: what
    /// [`Fake::serving_tls`](super::fake::Fake::serving_tls) takes.
    #[must_use]
    pub fn serving(&self, names: &[&str]) -> Arc<rustls::ServerConfig> {
        let key = KeyPair::generate().unwrap();
        let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
        let params = CertificateParams::new(names).unwrap();
        let certificate = params.signed_by(&key, &self.0).unwrap();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
        let cryptography = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(cryptography)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![certificate.der().clone()], key)
            .unwrap();
        Arc::new(config)
    }

    /// The TLS of a client that trusts this authority and no other: for a
    /// test that reads no environment.
    #[must_use]
    pub fn trusting(&self) -> rustls::ClientConfig {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(self.certificate()).unwrap();
        let cryptography = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ClientConfig::builder_with_provider(cryptography)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        config
    }
}
