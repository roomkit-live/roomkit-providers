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

    /// A directory of the directory, empty.
    fn directory(&self, name: &str) -> PathBuf {
        let directory = self.0.join(name);
        std::fs::create_dir(&directory).unwrap();
        directory
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

/// A server that shows `certificate`, whose key is `key`, and nothing else.
fn showing(certificate: &rcgen::Certificate, key: &KeyPair) -> Arc<ServerConfig> {
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
    let config = ServerConfig::builder_with_provider(ring())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![certificate.der().clone()], key)
        .unwrap();
    Arc::new(config)
}

// Where the client differs from Python's: a server's own certificate, one
// it signed itself and that says it is an authority, as `openssl req -x509`
// makes one, is not trusted when `SSL_CERT_FILE` names it: an authority is
// not taken for the certificate of a server. One that does not say it is an
// authority is trusted here.
#[test]
fn a_server_s_own_certificate_that_says_it_is_an_authority_is_not_trusted() {
    let scratch = Scratch::new("self-signed");
    let greeted_with_its_own = |is_ca: IsCa| {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(vec![HOST.to_owned()]).unwrap();
        params.is_ca = is_ca;
        let certificate = params.self_signed(&key).unwrap();
        let file = scratch.file("server.pem", &certificate.pem());
        let trusted = trust(&named(Some(&file), &[]), &ring(), unasked).unwrap();
        greeted(trusted, &showing(&certificate, &key), HOST)
    };
    let refused = greeted_with_its_own(IsCa::Ca(BasicConstraints::Unconstrained)).unwrap_err();
    assert_eq!(
        certificate_refused(&refused),
        Some(CertificateRefused::Untrusted)
    );
    assert_eq!(greeted_with_its_own(IsCa::NoCa), Ok(()));
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

/// Where a system would keep its authorities: `bundle`, and `directories`.
fn found(bundle: Option<&PathBuf>, directories: &[&PathBuf]) -> Found {
    Found {
        bundle: bundle.cloned(),
        directories: directories.iter().map(|kept| (*kept).clone()).collect(),
    }
}

/// How many authorities a client trusts on a system that keeps them at
/// `bundle` and `directories`.
fn kept(bundle: Option<&PathBuf>, directories: &[&PathBuf]) -> usize {
    kept_at(&found(bundle, directories)).unwrap().len()
}

/// The path of the store a client is not built for, on a system that keeps
/// its authorities at `bundle` and `directories`.
fn unread(bundle: Option<&PathBuf>, directories: &[&PathBuf]) -> PathBuf {
    match kept_at(&found(bundle, directories)).map(|roots| roots.len()) {
        Err(TlsUnset::SystemUnread(path)) => path,
        other => panic!("the store is read: {other:?}"),
    }
}

/// How many public roots are compiled in: more than a test's store holds.
fn compiled_in() -> usize {
    let compiled_in = webpki_roots::TLS_SERVER_ROOTS.len();
    assert!(compiled_in > 2);
    compiled_in
}

/// A block that says it is a certificate, and whose content is not written
/// as one is.
const CUT: &str = "-----BEGIN CERTIFICATE-----\nnot base64\n-----END CERTIFICATE-----\n";

/// A block written as a certificate is, which holds none.
const NO_CERTIFICATE: &str = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";

/// A path closed to its owner until this is dropped, when it opens again
/// for the test's directory to be removed.
#[cfg(unix)]
struct Closed(PathBuf);

#[cfg(unix)]
impl Closed {
    /// Closes `path`. `None` where a mode closes nothing to this process,
    /// which then cannot make a store it is refused: one that runs as
    /// root, or as the root of a namespace of its own.
    fn of(path: &Path) -> Option<Self> {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let closed = Self(path.to_owned());
        let refused = if path.is_dir() {
            std::fs::read_dir(path).is_err()
        } else {
            std::fs::File::open(path).is_err()
        };
        if !refused {
            println!("not run: a mode closes no file to this process");
        }
        refused.then_some(closed)
    }
}

#[cfg(unix)]
impl Drop for Closed {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700)).ok();
    }
}

// What follows reads what a system keeps on its disk, given as paths of a
// test's own directory: one test for each form a store may be found in, on
// its side of the line between a system that keeps no authority, which is
// given the compiled-in roots, and a store that is there and does not
// read, for which no client is built.

// No store found at all: the compiled-in roots.
#[test]
fn a_system_with_no_store_is_given_the_compiled_in_roots() {
    assert_eq!(kept(None, &[]), compiled_in());
}

// A store that is not there is no store: a bundle and a directory that
// were removed since they were found.
#[test]
fn a_system_store_that_is_not_there_is_no_store() {
    let scratch = Scratch::new("absent");
    let (bundle, directory) = (scratch.0.join("bundle.pem"), scratch.0.join("certs"));
    assert_eq!(kept(Some(&bundle), &[&directory]), compiled_in());
}

// A bundle that reads and holds nothing, or words and no block: the
// compiled-in roots.
#[test]
fn a_system_bundle_that_holds_no_block_is_given_the_compiled_in_roots() {
    let scratch = Scratch::new("empty-bundle");
    for nothing in ["", "\n\n", "# the authorities of this machine\n"] {
        let bundle = scratch.file("bundle.pem", nothing);
        assert_eq!(kept(Some(&bundle), &[]), compiled_in(), "{nothing:?}");
    }
}

// A directory with no authority filed under a hash name: the compiled-in
// roots, whatever it holds under other names, which is not read.
#[test]
fn a_system_directory_with_nothing_under_a_hash_name_is_given_the_compiled_in_roots() {
    let scratch = Scratch::new("unfiled");
    assert_eq!(kept(None, &[&scratch.0]), compiled_in());
    scratch.file("authority.pem", &authority("unfiled").pem());
    scratch.file("0a1b2c3d.pem", CUT);
    assert_eq!(kept(None, &[&scratch.0]), compiled_in());
}

// A store that holds an authority is the whole store, from its bundle as
// from its directories, each authority once: none of the compiled-in roots
// joins it.
#[test]
fn a_system_store_s_own_authorities_are_the_whole_store() {
    let scratch = Scratch::new("own");
    let (ours, theirs) = (authority("ours"), authority("theirs"));
    let bundle = scratch.file("bundle.pem", &ours.pem());
    assert_eq!(kept(Some(&bundle), &[]), 1);
    scratch.file("0a1b2c3d.0", &ours.pem());
    scratch.file("0a1b2c3d.1", &theirs.pem());
    assert_eq!(kept(None, &[&scratch.0]), 2);
    assert_eq!(kept(Some(&bundle), &[&scratch.0]), 2);
    let trusted = || Trust::Roots(kept_at(&found(Some(&bundle), &[])).unwrap());
    assert_eq!(greeted(trusted(), &server(&ours, HOST), HOST), Ok(()));
    let refused = greeted(trusted(), &server(&theirs, HOST), HOST).unwrap_err();
    assert_eq!(
        certificate_refused(&refused),
        Some(CertificateRefused::Untrusted)
    );
}

// A certificate the client cannot check a server against is left out, as a
// system's bundle may hold some: the authorities beside it are the store.
#[test]
fn a_certificate_that_is_no_authority_is_left_out_beside_one() {
    let scratch = Scratch::new("beside");
    let both = format!("{NO_CERTIFICATE}{}", authority("ours").pem());
    assert_eq!(kept(Some(&scratch.file("bundle.pem", &both)), &[]), 1);
}

// An entry filed under a hash name that is not there, a link to an
// authority that was removed, is passed over.
#[cfg(unix)]
#[test]
fn an_entry_of_a_system_directory_that_is_not_there_is_passed_over() {
    let scratch = Scratch::new("dangling");
    let (removed, link) = (scratch.0.join("removed.pem"), scratch.0.join("0a1b2c3d.0"));
    std::os::unix::fs::symlink(removed, link).unwrap();
    assert_eq!(kept(None, &[&scratch.0]), compiled_in());
    scratch.file("0a1b2c3d.1", &authority("ours").pem());
    assert_eq!(kept(None, &[&scratch.0]), 1);
}

// A bundle that is there and cannot be read: no client is built, and the
// refusal names the bundle.
#[cfg(unix)]
#[test]
fn a_system_bundle_that_cannot_be_read_is_refused() {
    let scratch = Scratch::new("closed-bundle");
    let bundle = scratch.file("bundle.pem", &authority("ours").pem());
    let Some(_closed) = Closed::of(&bundle) else {
        return;
    };
    assert_eq!(unread(Some(&bundle), &[]), bundle);
}

// A directory where the bundle is expected.
#[test]
fn a_system_bundle_that_is_a_directory_is_refused() {
    let scratch = Scratch::new("bundle-directory");
    assert_eq!(unread(Some(&scratch.0), &[]), scratch.0);
}

// A bundle one of whose blocks does not read, beside an authority or
// alone.
#[test]
fn a_system_bundle_with_a_block_that_does_not_read_is_refused() {
    let scratch = Scratch::new("cut-bundle");
    let ours = authority("ours").pem();
    let unended = "-----BEGIN CERTIFICATE-----\nAAAA\n";
    for written in [
        CUT.to_owned(),
        format!("{ours}{CUT}"),
        format!("{ours}{unended}"),
    ] {
        let bundle = scratch.file("bundle.pem", &written);
        assert_eq!(unread(Some(&bundle), &[]), bundle, "{written}");
    }
}

// A store that holds blocks, none of which gives an authority: a block
// that holds no certificate, a key, a block of a kind the client does not
// read. Such a system keeps something, which the compiled-in roots do not
// stand in for.
#[test]
fn a_system_store_that_holds_blocks_and_no_authority_is_refused() {
    let scratch = Scratch::new("no-authority");
    let key = KeyPair::generate().unwrap().serialize_pem();
    let other = "-----BEGIN TRUSTED CERTIFICATE-----\nAAAA\n-----END TRUSTED CERTIFICATE-----\n";
    for written in [NO_CERTIFICATE, key.as_str(), other] {
        let bundle = scratch.file("bundle.pem", written);
        assert_eq!(unread(Some(&bundle), &[]), bundle, "{written}");
        let entry = scratch.file("0a1b2c3d.0", written);
        assert_eq!(unread(None, &[&scratch.0]), entry, "{written}");
    }
}

// A directory that is there and cannot be read.
#[cfg(unix)]
#[test]
fn a_system_directory_that_cannot_be_read_is_refused() {
    let scratch = Scratch::new("closed-directory");
    let directory = scratch.directory("certs");
    std::fs::write(directory.join("0a1b2c3d.0"), authority("ours").pem()).unwrap();
    let Some(_closed) = Closed::of(&directory) else {
        return;
    };
    assert_eq!(unread(None, &[&directory]), directory);
}

// A file where a directory is expected.
#[test]
fn a_system_directory_that_is_a_file_is_refused() {
    let scratch = Scratch::new("directory-file");
    let file = scratch.file("certs", &authority("ours").pem());
    assert_eq!(unread(None, &[&file]), file);
}

// An entry filed under a hash name that cannot be read: the refusal names
// the entry, beside an authority that reads as alone.
#[cfg(unix)]
#[test]
fn an_entry_of_a_system_directory_that_cannot_be_read_is_refused() {
    let scratch = Scratch::new("closed-entry");
    scratch.file("0a1b2c3d.0", &authority("ours").pem());
    let entry = scratch.file("0a1b2c3d.1", &authority("theirs").pem());
    let Some(_closed) = Closed::of(&entry) else {
        return;
    };
    assert_eq!(unread(None, &[&scratch.0]), entry);
}

// An entry filed under a hash name one of whose blocks does not read, and
// one that is a directory.
#[test]
fn an_entry_of_a_system_directory_that_does_not_read_is_refused() {
    let scratch = Scratch::new("cut-entry");
    scratch.file("0a1b2c3d.0", &authority("ours").pem());
    let entry = scratch.file("0a1b2c3d.1", CUT);
    assert_eq!(unread(None, &[&scratch.0]), entry);
    std::fs::remove_file(&entry).unwrap();
    let entry = scratch.directory("0a1b2c3d.1");
    assert_eq!(unread(None, &[&scratch.0]), entry);
}

// The refusal names the path of the store, and nothing a file holds.
#[test]
fn a_system_store_that_does_not_read_is_named_by_its_path() {
    let unset = TlsUnset::SystemUnread(PathBuf::from("/etc/ssl/certs/ca-certificates.crt"));
    assert_eq!(
        unset.reason(),
        "the system's certificate store at /etc/ssl/certs/ca-certificates.crt cannot be read"
    );
    assert_eq!(unset.to_string(), unset.reason());
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
