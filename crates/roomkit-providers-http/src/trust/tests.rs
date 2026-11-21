//! What a server's certificate is checked against, for each thing the
//! environment may name. No test sets a variable: each gives what the
//! variables would name, and an authority made when it runs.

use std::ffi::OsString;
use std::path::PathBuf;

use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, DnType, IsCa, KeyPair};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use rustls::{ClientConnection, ServerConfig, ServerConnection};

use super::*;

/// The `supported_versions` extension of a hello that offers TLS 1.3,
/// then TLS 1.2: its type, its length, then the two versions.
const TLS_1_3_THEN_1_2: [u8; 9] = [0x00, 0x2b, 0x00, 0x05, 0x04, 0x03, 0x04, 0x03, 0x03];

/// The name the tests' server is asked under.
const HOST: &str = "service.example";

fn ring() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// An authority made for a test: its key lives as long as the test.
fn authority(name: &str) -> CertifiedIssuer<'static, KeyPair> {
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.distinguished_name.push(DnType::CommonName, name);
    CertifiedIssuer::self_signed(params, KeyPair::generate().unwrap()).unwrap()
}

/// A server whose certificate, for `name`, `authority` signed.
fn server(authority: &CertifiedIssuer<'static, KeyPair>, name: &str) -> Arc<ServerConfig> {
    let key = KeyPair::generate().unwrap();
    let params = CertificateParams::new(vec![name.to_owned()]).unwrap();
    let certificate = params.signed_by(&key, authority).unwrap();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
    let config = ServerConfig::builder_with_provider(ring())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![certificate.der().clone()], key)
        .unwrap();
    Arc::new(config)
}

/// A directory of a test's own, removed with it.
struct Scratch(PathBuf);

impl Scratch {
    fn new(test: &str) -> Self {
        let name = format!("roomkit-providers-trust-{}-{test}", std::process::id());
        let directory = env::temp_dir().join(name);
        std::fs::create_dir_all(&directory).unwrap();
        Self(directory)
    }

    /// A file of the directory that holds `content`.
    fn file(&self, name: &str, content: &str) -> PathBuf {
        let file = self.0.join(name);
        std::fs::write(&file, content).unwrap();
        file
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// What the environment names when `SSL_CERT_FILE` is `file` and
/// `SSL_CERT_DIR` is `directories`.
fn named(file: Option<&PathBuf>, directories: &[&PathBuf]) -> Named {
    Named {
        file: file.cloned(),
        directories: directories.iter().map(|named| (*named).clone()).collect(),
    }
}

/// A system that must not be asked: the environment decided.
fn unasked(_: &Arc<CryptoProvider>) -> Result<Trust, TlsUnset> {
    panic!("the system is not asked when the environment names its authorities")
}

/// The store `trust` decided, which is no verifier of the system.
fn roots(trust: Result<Trust, TlsUnset>) -> RootCertStore {
    match trust.unwrap() {
        Trust::Roots(roots) => roots,
        Trust::Verifier(_) => panic!("the trust is a store"),
    }
}

/// What a client that trusts `trust` says of `server`, asked under `name`:
/// the handshake of the two is carried here, over no socket.
fn greeted(trust: Trust, server: &Arc<ServerConfig>, name: &str) -> Result<(), rustls::Error> {
    let config = Arc::new(configured(ring(), trust).unwrap());
    let name = ServerName::try_from(name.to_owned()).unwrap();
    let mut client = ClientConnection::new(config, name).unwrap();
    let mut server = ServerConnection::new(Arc::clone(server)).unwrap();
    while client.is_handshaking() || server.is_handshaking() {
        let mut sent = Vec::new();
        client.write_tls(&mut sent).unwrap();
        server.read_tls(&mut sent.as_slice()).unwrap();
        // A server that could not read the client was told why it left.
        if server.process_new_packets().is_err() {
            break;
        }
        let mut answered = Vec::new();
        server.write_tls(&mut answered).unwrap();
        client.read_tls(&mut answered.as_slice()).unwrap();
        client.process_new_packets()?;
    }
    Ok(())
}

#[test]
fn the_tls_configuration_asks_for_http_1_1() {
    let config = configured(ring(), Trust::Roots(public_roots())).unwrap();
    assert_eq!(config.alpn_protocols, [b"http/1.1".to_vec()]);
}

/// The versions are read where a server reads them: in the hello a
/// client of the configuration writes first, which no socket carries
/// here.
#[test]
fn a_client_of_the_tls_configuration_offers_tls_1_3_and_tls_1_2() {
    let config = Arc::new(configured(ring(), Trust::Roots(public_roots())).unwrap());
    let server = ServerName::try_from(HOST).unwrap();
    let mut client = ClientConnection::new(config, server).unwrap();
    let mut hello = Vec::new();
    client.write_tls(&mut hello).unwrap();
    let offered = hello
        .windows(TLS_1_3_THEN_1_2.len())
        .any(|bytes| bytes == TLS_1_3_THEN_1_2);
    assert!(offered, "{hello:02x?}");
}

// Python: httpx2 create_ssl_context reads a variable with `os.environ.get`
// and takes an empty value for none.
#[test]
fn a_variable_set_to_nothing_names_nothing() {
    let set = |value: &str| Some(OsString::from(value));
    assert_eq!(Named::of(None, None), Named::default());
    assert_eq!(Named::of(set(""), set("")), Named::default());
    let separator = if cfg!(windows) { ";" } else { ":" };
    let listed = format!("/first{separator}{separator}/second");
    let both = Named::of(set("/authorities.pem"), set(&listed));
    assert_eq!(both.file, Some(PathBuf::from("/authorities.pem")));
    let directories = [PathBuf::from("/first"), PathBuf::from("/second")];
    assert_eq!(both.directories, directories);
}

// The file `SSL_CERT_FILE` names is the whole store: a server an authority
// of the file signed is trusted, one another authority signed is not, and
// the system is not asked. The file is read before the directories, which
// are then not read at all.
#[test]
fn the_file_the_environment_names_is_the_whole_store() {
    let scratch = Scratch::new("file");
    let (ours, theirs) = (authority("ours"), authority("theirs"));
    let file = scratch.file("authorities.pem", &ours.pem());
    scratch.file("0a1b2c3d.0", &theirs.pem());
    let trusted = || trust(&named(Some(&file), &[&scratch.0]), &ring(), unasked);
    assert_eq!(roots(trusted()).len(), 1);
    assert_eq!(
        greeted(trusted().unwrap(), &server(&ours, HOST), HOST),
        Ok(())
    );
    let refused = greeted(trusted().unwrap(), &server(&theirs, HOST), HOST).unwrap_err();
    assert_eq!(
        certificate_refused(&refused),
        Some(CertificateRefused::Untrusted)
    );
    // A certificate for another name is refused, whoever signed it.
    let other = greeted(trusted().unwrap(), &server(&ours, "other.example"), HOST).unwrap_err();
    assert_eq!(
        certificate_refused(&other),
        Some(CertificateRefused::OtherName)
    );
}

// Python: ssl.create_default_context(cafile=...) raises for a file that is
// not there and for one that holds no certificate, when the client is built.
#[test]
fn a_file_that_does_not_read_or_holds_no_certificate_is_refused() {
    let scratch = Scratch::new("unread");
    let refused = |file: &PathBuf| {
        let trusted = trust(&named(Some(file), &[]), &ring(), unasked);
        trusted.map(|_| ()).unwrap_err()
    };
    assert_eq!(
        refused(&scratch.0.join("missing.pem")),
        TlsUnset::FileUnread
    );
    assert_eq!(refused(&scratch.file("empty.pem", "")), TlsUnset::FileEmpty);
    let key = KeyPair::generate().unwrap().serialize_pem();
    assert_eq!(refused(&scratch.file("key.pem", &key)), TlsUnset::FileEmpty);
    let cut = "-----BEGIN CERTIFICATE-----\nnot base64\n-----END CERTIFICATE-----\n";
    assert_eq!(refused(&scratch.file("cut.pem", cut)), TlsUnset::FileUnread);
    assert_eq!(
        TlsUnset::FileUnread.reason(),
        "the file SSL_CERT_FILE names cannot be read"
    );
}

// The directories `SSL_CERT_DIR` names give what is filed there under the
// name OpenSSL looks an authority up by, and nothing else: a certificate
// under another name is not read, as OpenSSL does not find it. A directory
// that holds none leaves no authority, and the compiled-in roots do not
// stand in for it.
#[test]
fn a_directory_gives_what_is_filed_under_a_hash_name() {
    let scratch = Scratch::new("directory");
    let (filed, unfiled) = (authority("filed"), authority("unfiled"));
    scratch.file("0a1b2c3d.0", &filed.pem());
    scratch.file("authority.pem", &unfiled.pem());
    scratch.file("0A1B2C3D.0", &unfiled.pem());
    scratch.file("0a1b2c3d.pem", &unfiled.pem());
    let nowhere = scratch.0.join("nowhere");
    let trusted = || trust(&named(None, &[&nowhere, &scratch.0]), &ring(), unasked);
    assert_eq!(roots(trusted()).len(), 1);
    assert_eq!(
        greeted(trusted().unwrap(), &server(&filed, HOST), HOST),
        Ok(())
    );
    let refused = greeted(trusted().unwrap(), &server(&unfiled, HOST), HOST).unwrap_err();
    assert_eq!(
        certificate_refused(&refused),
        Some(CertificateRefused::Untrusted)
    );
    let none = trust(&named(None, &[&nowhere]), &ring(), unasked);
    assert!(roots(none).is_empty());
}

// The system is asked only when the environment names nothing.
#[test]
fn the_system_is_asked_when_the_environment_names_nothing() {
    let ours = authority("ours");
    let system = |_: &Arc<CryptoProvider>| {
        let certificates = vec![ours.der().clone()];
        Ok(Trust::Roots(or_compiled_in(store_of(certificates))))
    };
    let trusted = trust(&Named::default(), &ring(), system).unwrap();
    assert_eq!(greeted(trusted, &server(&ours, HOST), HOST), Ok(()));
}

// A system that keeps no authority is given the compiled-in roots; one that
// keeps its own is given none of them.
#[test]
fn the_compiled_in_roots_stand_in_for_an_empty_system_and_join_no_other() {
    let compiled_in = webpki_roots::TLS_SERVER_ROOTS.len();
    assert!(compiled_in > 0);
    assert_eq!(or_compiled_in(store_of(Vec::new())).len(), compiled_in);
    let ours = authority("ours");
    let own = or_compiled_in(store_of(vec![ours.der().clone(), ours.der().clone()]));
    assert_eq!(own.len(), 1);
}

/// An error that only carries another, as an HTTP client's does.
#[derive(Debug)]
struct Carrying(std::io::Error);

impl std::fmt::Display for Carrying {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a connection failed")
    }
}

impl Error for Carrying {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}

// A refused certificate is found under the errors that carry it, inside the
// I/O errors TLS and the HTTP client hand it on in, one inside the other,
// and told from any other failure.
#[test]
fn a_refused_certificate_is_told_from_any_other_failure() {
    let carried = |error: rustls::Error| {
        let from_tls = std::io::Error::new(std::io::ErrorKind::InvalidData, error);
        let carrying = Carrying(std::io::Error::other(from_tls));
        certificate_refused(&carrying)
    };
    let invalid = |why: CertificateError| carried(rustls::Error::InvalidCertificate(why));
    assert_eq!(
        invalid(CertificateError::UnknownIssuer),
        Some(CertificateRefused::Untrusted)
    );
    assert_eq!(
        invalid(CertificateError::BadSignature),
        Some(CertificateRefused::Untrusted)
    );
    assert_eq!(
        invalid(CertificateError::NotValidForName),
        Some(CertificateRefused::OtherName)
    );
    assert_eq!(
        invalid(CertificateError::Expired),
        Some(CertificateRefused::OutOfDate)
    );
    assert_eq!(
        invalid(CertificateError::NotValidYet),
        Some(CertificateRefused::OutOfDate)
    );
    assert_eq!(carried(rustls::Error::DecryptError), None);
    let closed = Carrying(std::io::Error::other("the connection was closed"));
    assert_eq!(certificate_refused(&closed), None);
}
