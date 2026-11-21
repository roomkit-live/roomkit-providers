//! The store of authorities a system keeps on its disk, where it has no
//! verifier of its own: its bundle file and its directories of hashed
//! names, as OpenSSL reads them.
//!
//! A system that keeps no authority is given the public roots compiled in.
//! A store that is there and does not read is given nothing in its place:
//! it is refused, by its path.

use std::path::{Path, PathBuf};

use rustls::RootCertStore;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;

use super::{TlsUnset, is_filed_by_hash, or_compiled_in, store_of};

/// Where a system keeps its authorities on its disk.
#[derive(Debug, Default)]
pub(super) struct Found {
    /// Its bundle: one file of authorities.
    pub(super) bundle: Option<PathBuf>,
    /// Its directories of authorities, each filed under a hashed name.
    pub(super) directories: Vec<PathBuf>,
}

impl Found {
    /// Where this system keeps its authorities: what `openssl-probe` finds,
    /// the places OpenSSL looks for them.
    #[cfg(all(unix, not(target_vendor = "apple")))]
    pub(super) fn on_the_disk() -> Self {
        let found = openssl_probe::probe();
        Self {
            bundle: found.cert_file,
            directories: found.cert_dir,
        }
    }

    /// Where this system keeps its authorities: nowhere on its disk, where
    /// it keeps its store elsewhere.
    #[cfg(not(all(unix, not(target_vendor = "apple"))))]
    pub(super) fn on_the_disk() -> Self {
        Self::default()
    }
}

/// Whether `error` says that a path is not there.
fn is_absent(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotFound
}

/// What a file of the system's store holds: its certificates, and whether
/// it holds a block at all, of a certificate or of anything else.
///
/// # Errors
///
/// A file that is there and does not read: one the process may not open,
/// a directory, a block that is not written as one is.
fn held_by(file: &Path) -> Result<(Vec<CertificateDer<'static>>, bool), TlsUnset> {
    let unread = || TlsUnset::SystemUnread(file.to_owned());
    let written = match std::fs::read(file) {
        Ok(written) => written,
        // A file that is not there holds nothing: a link to an authority
        // that was removed.
        Err(error) if is_absent(&error) => return Ok((Vec::new(), false)),
        Err(_) => return Err(unread()),
    };
    let certificates = CertificateDer::pem_slice_iter(&written).collect::<Result<_, _>>();
    // A block starts where a line does, whatever it says it holds.
    let mut lines = written.split(|byte| matches!(byte, b'\n' | b'\r'));
    let holds_a_block = lines.any(|line| line.starts_with(b"-----BEGIN "));
    Ok((certificates.map_err(|_| unread())?, holds_a_block))
}

/// The files of `directory`, a directory of the system's store, that are
/// named as OpenSSL names an authority, in the order of their names.
///
/// # Errors
///
/// A directory that is there and does not read: one the process may not
/// list, a file.
fn filed_under(directory: &Path) -> Result<Vec<PathBuf>, TlsUnset> {
    let unread = |_| TlsUnset::SystemUnread(directory.to_owned());
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if is_absent(&error) => return Ok(Vec::new()),
        Err(error) => return Err(unread(error)),
    };
    let mut filed = Vec::new();
    for entry in entries {
        let path = entry.map_err(unread)?.path();
        if is_filed_by_hash(&path) {
            filed.push(path);
        }
    }
    filed.sort();
    Ok(filed)
}

/// What a client checks a certificate against on a system that keeps its
/// authorities at `found`: those of its bundle and those filed in its
/// directories, or the public roots compiled in when it keeps none.
///
/// The compiled-in roots are for a system that keeps no authority: no
/// store found, a store that is no longer there, a bundle that holds no
/// block, a directory with nothing filed under a hash name. They never
/// stand in for a store that is there and does not read.
///
/// # Errors
///
/// A store that is there and does not read, named by its path: a bundle, a
/// directory or a file under a hash name that the process may not read,
/// that is a directory where a file is expected or a file where a
/// directory is, or one of whose blocks is not written as one is; and a
/// store that holds blocks, none of which gives an authority. A
/// certificate that is no authority beside one that is is left out, as a
/// system's bundle may hold some.
pub(super) fn kept_at(found: &Found) -> Result<RootCertStore, TlsUnset> {
    let mut files: Vec<PathBuf> = found.bundle.iter().cloned().collect();
    for directory in &found.directories {
        files.extend(filed_under(directory)?);
    }
    let mut certificates = Vec::new();
    // The first file that holds a block, should none give an authority.
    let mut holding = None;
    for file in files {
        let (held, holds_a_block) = held_by(&file)?;
        certificates.extend(held);
        if holds_a_block && holding.is_none() {
            holding = Some(file);
        }
    }
    let roots = store_of(certificates);
    match holding {
        Some(file) if roots.is_empty() => Err(TlsUnset::SystemUnread(file)),
        Some(_) | None => Ok(or_compiled_in(roots)),
    }
}
