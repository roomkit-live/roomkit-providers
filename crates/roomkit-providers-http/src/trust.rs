//! What a server's certificate is checked against. It is decided once, when
//! a client's TLS configuration is made, from the environment and the
//! system, as the client of `RoomKit`'s Python providers decides it:
//!
//! - `SSL_CERT_FILE` names a file: the authorities of that file, and no
//!   other;
//! - else `SSL_CERT_DIR` names directories: the authorities filed there
//!   under the names OpenSSL looks them up by, and no other;
//! - else the system: its own verifier on macOS and Windows, the store of
//!   authorities it keeps on its disk elsewhere.
//!
//! A variable set to nothing is not set. One point is this crate's own: a
//! system that keeps no authority at all, a container without a certificate
//! store, is given the public roots compiled in. They are never added to a
//! store that holds an authority of its own, and never stand in for what a
//! variable names.

use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::client::danger::ServerCertVerifier;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;
use rustls::{CertificateError, ClientConfig, RootCertStore};

/// The variable that names a file of authorities.
const FILE: &str = "SSL_CERT_FILE";

/// The variable that names directories of authorities.
const DIRECTORIES: &str = "SSL_CERT_DIR";

/// Why the TLS configuration of a client could not be made.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsUnset {
    /// The cryptography provider has no usable protocol version.
    Cryptography,
    /// The file `SSL_CERT_FILE` names does not read as certificates.
    FileUnread,
    /// The file `SSL_CERT_FILE` names holds no certificate.
    FileEmpty,
    /// The system's certificate verifier could not be made.
    Verifier,
}

impl TlsUnset {
    /// Why, in words that follow `the provider cannot be built: `.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Cryptography => "TLS is not set",
            Self::FileUnread => "the file SSL_CERT_FILE names cannot be read",
            Self::FileEmpty => "the file SSL_CERT_FILE names holds no certificate",
            Self::Verifier => "the system's certificate verifier cannot be used",
        }
    }
}

impl std::fmt::Display for TlsUnset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason())
    }
}

impl Error for TlsUnset {}

/// What the environment names of the authorities to trust.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Named {
    /// The file `SSL_CERT_FILE` names.
    pub(crate) file: Option<PathBuf>,
    /// The directories `SSL_CERT_DIR` names, separated as the system
    /// separates the entries of a path.
    pub(crate) directories: Vec<PathBuf>,
}

impl Named {
    /// What the environment names now.
    fn of_the_environment() -> Self {
        Self::of(env::var_os(FILE), env::var_os(DIRECTORIES))
    }

    /// What the two variables name: one set to nothing names nothing.
    pub(crate) fn of(file: Option<OsString>, directories: Option<OsString>) -> Self {
        let set = |value: Option<OsString>| value.filter(|value| !value.is_empty());
        let directories = set(directories).map(|named| {
            let named = env::split_paths(&named);
            named.filter(|path| !path.as_os_str().is_empty()).collect()
        });
        Self {
            file: set(file).map(PathBuf::from),
            directories: directories.unwrap_or_default(),
        }
    }
}

/// What a server's certificate is checked against.
pub(crate) enum Trust {
    /// A store of authorities, which the client checks a certificate
    /// against itself.
    Roots(RootCertStore),
    /// The system's own verifier.
    Verifier(Arc<dyn ServerCertVerifier>),
}

/// TLS with ring and HTTP/1.1 ALPN, a server's certificate checked against
/// what the environment and the system name when this is called (see the
/// module).
///
/// # Errors
///
/// The cryptography provider has no usable protocol version, the file
/// `SSL_CERT_FILE` names does not read or holds no certificate, or the
/// system's verifier could not be made.
pub(crate) fn tls_config() -> Result<ClientConfig, TlsUnset> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let trust = trust(&Named::of_the_environment(), &provider, system)?;
    configured(provider, trust)
}

/// The configuration that checks a server's certificate against `trust`.
pub(crate) fn configured(
    provider: Arc<CryptoProvider>,
    trust: Trust,
) -> Result<ClientConfig, TlsUnset> {
    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| TlsUnset::Cryptography)?;
    let builder = match trust {
        Trust::Roots(roots) => builder.with_root_certificates(roots),
        Trust::Verifier(verifier) => builder
            .dangerous()
            .with_custom_certificate_verifier(verifier),
    };
    let mut config = builder.with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

/// What a certificate is checked against, given what the environment
/// `named`: its file, else its directories, else what `system` gives.
pub(crate) fn trust(
    named: &Named,
    provider: &Arc<CryptoProvider>,
    system: impl FnOnce(&Arc<CryptoProvider>) -> Result<Trust, TlsUnset>,
) -> Result<Trust, TlsUnset> {
    if let Some(file) = &named.file {
        let held = sections_of(file).and_then(Iterator::collect::<Result<Vec<_>, _>>);
        let roots = store_of(held.map_err(|_| TlsUnset::FileUnread)?);
        return if roots.is_empty() {
            Err(TlsUnset::FileEmpty)
        } else {
            Ok(Trust::Roots(roots))
        };
    }
    if named.directories.is_empty() {
        return system(provider);
    }
    // A directory that holds none leaves the store empty: no certificate
    // is trusted then, as with OpenSSL.
    let filed = named.directories.iter().flat_map(|named| filed_in(named));
    Ok(Trust::Roots(store_of(filed.collect())))
}

/// The certificate sections of `file`, a PEM file.
fn sections_of(
    file: &Path,
) -> Result<
    impl Iterator<Item = Result<CertificateDer<'static>, rustls::pki_types::pem::Error>>,
    rustls::pki_types::pem::Error,
> {
    CertificateDer::pem_file_iter(file)
}

/// The certificates of `file` that read: for a file of the system's store,
/// of which a part that does not read is passed over.
fn readable_of(file: &Path) -> Vec<CertificateDer<'static>> {
    let sections = sections_of(file).into_iter().flatten();
    sections.filter_map(Result::ok).collect()
}

/// Whether `path` is named as OpenSSL names an authority in a directory:
/// the eight hexadecimal digits of its subject's hash, a dot and a number.
fn is_filed_by_hash(path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str());
    let Some((hash, number)) = name.and_then(|name| name.split_once('.')) else {
        return false;
    };
    let hexadecimal = |byte: u8| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte);
    hash.len() == 8
        && hash.bytes().all(hexadecimal)
        && !number.is_empty()
        && number.bytes().all(|byte| byte.is_ascii_digit())
}

/// The certificates filed in `directory` under the names OpenSSL looks an
/// authority up by. A directory or a file that does not read gives none.
fn filed_in(directory: &Path) -> Vec<CertificateDer<'static>> {
    let entries = std::fs::read_dir(directory).into_iter().flatten();
    let mut filed: Vec<PathBuf> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| is_filed_by_hash(path))
        .collect();
    filed.sort();
    filed.iter().flat_map(|file| readable_of(file)).collect()
}

/// A store of `certificates`, each once: those that are no authority a
/// certificate can be checked against are left out.
fn store_of(mut certificates: Vec<CertificateDer<'static>>) -> RootCertStore {
    certificates.sort_unstable_by(|one, other| one.as_ref().cmp(other.as_ref()));
    certificates.dedup();
    let mut roots = RootCertStore::empty();
    roots.add_parsable_certificates(certificates);
    roots
}

/// `roots`, the system's, or the public roots compiled in when the system
/// gave none: they are never added to a store that holds an authority.
pub(crate) fn or_compiled_in(roots: RootCertStore) -> RootCertStore {
    if roots.is_empty() {
        public_roots()
    } else {
        roots
    }
}

/// The public web PKI roots compiled in.
pub(crate) fn public_roots() -> RootCertStore {
    RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    }
}

/// Whether the system verifies a certificate itself: macOS and Windows.
/// Elsewhere it keeps a store of authorities on its disk, which the client
/// reads into its own.
const THE_SYSTEM_VERIFIES: bool = cfg!(any(target_vendor = "apple", windows));

/// What the system trusts: its own verifier where it has one, else the
/// authorities it keeps, or the compiled-in roots when it keeps none.
fn system(provider: &Arc<CryptoProvider>) -> Result<Trust, TlsUnset> {
    if THE_SYSTEM_VERIFIES {
        let verifier = rustls_platform_verifier::Verifier::new(Arc::clone(provider));
        let verifier = verifier.map_err(|_| TlsUnset::Verifier)?;
        return Ok(Trust::Verifier(Arc::new(verifier)));
    }
    Ok(Trust::Roots(or_compiled_in(store_of(kept_by_the_system()))))
}

/// The authorities the system keeps on its disk: those of its bundle file
/// and those filed in its directories, where OpenSSL looks for them.
#[cfg(all(unix, not(target_vendor = "apple")))]
fn kept_by_the_system() -> Vec<CertificateDer<'static>> {
    let found = openssl_probe::probe();
    let bundled = found.cert_file.iter().flat_map(|file| readable_of(file));
    let filed = found.cert_dir.iter().flat_map(|named| filed_in(named));
    bundled.chain(filed).collect()
}

/// The authorities the system keeps on its disk: none where it keeps its
/// store elsewhere.
#[cfg(not(all(unix, not(target_vendor = "apple"))))]
fn kept_by_the_system() -> Vec<CertificateDer<'static>> {
    Vec::new()
}

/// What a client refused of the certificate a server showed it.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertificateRefused {
    /// No authority the client trusts signed it, or it fails another check.
    Untrusted,
    /// It is for another host name than the one the client asked.
    OtherName,
    /// It has expired, or is not valid yet.
    OutOfDate,
}

/// What `error`, the failure of a connection, says the client refused of
/// the server's certificate; `None` for a failure that is no such refusal.
///
/// For a provider crate: the error of its HTTP client, whose causes are
/// searched for the refusal of this crate's TLS.
#[doc(hidden)]
#[must_use]
pub fn certificate_refused(error: &(dyn Error + 'static)) -> Option<CertificateRefused> {
    let mut cause = Some(error);
    while let Some(found) = cause {
        if let Some(rustls::Error::InvalidCertificate(why)) = failure_of_tls(found) {
            return Some(refusal_of(why));
        }
        cause = found.source();
    }
    None
}

/// The failure of TLS `error` is or carries. TLS hands its failure on
/// inside an I/O error, and an HTTP client hands that one on inside
/// another: neither names what it carries as its source.
fn failure_of_tls<'e>(error: &'e (dyn Error + 'static)) -> Option<&'e rustls::Error> {
    let mut carried = error;
    loop {
        if let Some(failure) = carried.downcast_ref::<rustls::Error>() {
            return Some(failure);
        }
        carried = carried.downcast_ref::<std::io::Error>()?.get_ref()?;
    }
}

/// How a refused certificate is told.
fn refusal_of(why: &CertificateError) -> CertificateRefused {
    let other_name = matches!(
        why,
        CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. }
    );
    let out_of_date = matches!(
        why,
        CertificateError::Expired
            | CertificateError::ExpiredContext { .. }
            | CertificateError::NotValidYet
            | CertificateError::NotValidYetContext { .. }
    );
    if other_name {
        CertificateRefused::OtherName
    } else if out_of_date {
        CertificateRefused::OutOfDate
    } else {
        CertificateRefused::Untrusted
    }
}

#[cfg(test)]
mod tests;
