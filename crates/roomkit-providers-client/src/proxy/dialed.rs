//! A proxy as a client reaches it: where it is dialed, and what it is
//! given.
//!
//! The two are kept apart from the moment a variable is read, and no text
//! of a variable is read as a URL a second time. The HTTP client's reader
//! of a URL is not Python's: it ends an authority at a backslash, where
//! Python's reads on to the last `@`. User information written back into a
//! URL could so name another host than the variable does. Here:
//!
//! - the address is the proxy's scheme, host and port, each written into a
//!   URL by the setter of that part, which reads it as that part alone;
//! - the credentials are the user information as Python's client reads it,
//!   handed to the HTTP client as a user and a password.

use std::net::IpAddr;

use super::pattern::{self, Host, Read, Unread};
use crate::failure::Failure;

/// What a proxy is given as Basic credentials: the user information of its
/// URL, as Python's client reads it.
#[derive(Clone)]
pub(crate) struct Credentials {
    /// The user information as the variable writes it: what stands before
    /// the last `@` of the proxy's authority.
    pub(crate) written: String,
    /// The user: what stands before the first colon, decoded once.
    pub(crate) user: String,
    /// The password: what follows that colon, decoded once.
    pub(crate) password: String,
}

impl Credentials {
    /// What `written`, the user information of a proxy's URL, gives the
    /// proxy: nothing when it names neither a user nor a password.
    fn of(written: &str) -> Option<Self> {
        let (user, password) = written.split_once(':').unwrap_or((written, ""));
        let (user, password) = (decoded(user), decoded(password));
        (!user.is_empty() || !password.is_empty()).then(|| Self {
            written: written.to_owned(),
            user,
            password,
        })
    }
}

/// `written` as Python's `unquote` reads it: a `%` before two hexadecimal
/// digits is the byte they write, any other is itself, and what is no
/// UTF-8 afterwards is replaced.
fn decoded(written: &str) -> String {
    let digit = |digit: u8| char::from(digit).to_digit(16);
    let mut bytes = Vec::with_capacity(written.len());
    let mut rest = written.as_bytes();
    while let Some((first, after)) = rest.split_first() {
        let digits = after.split_first_chunk::<2>().filter(|_| *first == b'%');
        let encoded = digits.and_then(|([high, low], after)| {
            let byte = u8::try_from(digit(*high)? * 16 + digit(*low)?).ok()?;
            Some((byte, after))
        });
        let (byte, after) = encoded.unwrap_or((*first, after));
        bytes.push(byte);
        rest = after;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// A proxy the environment names.
#[derive(Clone)]
pub(crate) struct Proxy {
    /// Where it is dialed: its scheme, its host and its port, and nothing
    /// else of its URL.
    pub(crate) address: reqwest::Url,
    /// What it is given, when its URL carries user information.
    pub(crate) credentials: Option<Credentials>,
}

/// Whether a host name may hold `sign`: a letter, a digit, a hyphen, a dot
/// or an underscore.
fn of_a_host_name(sign: char) -> bool {
    sign.is_ascii_alphanumeric() || matches!(sign, '-' | '.' | '_')
}

/// Where the proxy `read` names is dialed, `read` naming an `http` or an
/// `https` proxy.
///
/// # Errors
///
/// A proxy this client does not dial: one without a host, one whose port
/// is past the last, and one whose name holds another sign than a host
/// name's, as the variable writes it or as the HTTP client writes it
/// again. The HTTP client reads an address twice, with two readers that
/// do not take every other sign alike: it would end the host at one,
/// decode another, and send around a proxy whose name it does not read.
fn address_of(read: &Read<'_>) -> Result<reqwest::Url, Unread> {
    let of_its_scheme = match read.scheme.as_str() {
        "https" => "https://proxy.invalid/",
        _ => "http://proxy.invalid/",
    };
    let mut address = reqwest::Url::parse(of_its_scheme).map_err(|_| Unread)?;
    match &read.host {
        Host::Name(name) | Host::International(name) => {
            let written = |sign: char| !sign.is_ascii() || of_a_host_name(sign);
            if !name.chars().all(written) {
                return Err(Unread);
            }
            address.set_host(Some(name)).map_err(|_| Unread)?;
            let dialed = address.host_str().unwrap_or_default();
            if !dialed.chars().all(of_a_host_name) {
                return Err(Unread);
            }
        }
        Host::V6(dialed, _) => {
            let dialed = IpAddr::V6(*dialed);
            address.set_ip_host(dialed).map_err(|()| Unread)?;
        }
        Host::Absent => return Err(Unread),
    }
    let port = read.port.map(u16::try_from).transpose();
    let port = port.map_err(|_| Unread)?;
    address.set_port(port).map_err(|()| Unread)?;
    Ok(address)
}

/// The proxy `written` names: its address, and the credentials its user
/// information gives it. Its path is no part of it.
///
/// # Errors
///
/// The failure of a provider that cannot be built, which repeats nothing
/// of `written`: a text Python's client refuses, a proxy that is neither
/// `http` nor `https`, and a proxy Python's client could not reach.
pub(super) fn proxy_of(written: &str) -> Result<Proxy, Failure> {
    let unread = |_| Failure::unbuilt("a proxy the environment names does not read as a URL");
    let read = pattern::read(written).map_err(unread)?;
    match read.scheme.as_str() {
        "http" | "https" => {}
        "socks5" | "socks5h" => {
            return Err(Failure::unbuilt(
                "the environment names a SOCKS proxy, which is not supported",
            ));
        }
        _ => {
            return Err(Failure::unbuilt(
                "a proxy the environment names is neither an http nor an https proxy",
            ));
        }
    }
    Ok(Proxy {
        address: address_of(&read).map_err(unread)?,
        credentials: Credentials::of(read.user_information),
    })
}
