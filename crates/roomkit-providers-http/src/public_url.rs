//! Check an outbound HTTP destination before handing it to a transport.

mod addresses;
mod authority;
mod host;
mod words;

use std::net::{IpAddr, ToSocketAddrs};

use self::authority::Authority;
use self::host::Host;
use self::words::{quoted, tuple};

/// The names of this machine a URL may not name.
const LOCALHOST_NAMES: [&str; 3] = ["localhost", "ip6-localhost", "ip6-loopback"];

/// The keyword arguments of Python's `validate_public_url`: `allowed_schemes=("http",
/// "https")` and `resolve_dns=True`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidatePublicUrlOptions<'a> {
    /// Schemes accepted by this caller, in lower case.
    pub allowed_schemes: &'a [&'a str],
    /// Resolve a hostname and inspect every address it returns. Without it
    /// the URL is checked as it is written only, and a name is accepted
    /// unchecked, with the exceptions [`validate_public_url`] lists.
    pub resolve_dns: bool,
}

impl Default for ValidatePublicUrlOptions<'_> {
    fn default() -> Self {
        Self {
            allowed_schemes: &["http", "https"],
            resolve_dns: true,
        }
    }
}

/// Check that an outbound URL names a public destination, and return its
/// original spelling: Python's `validate_public_url(url, *,
/// allowed_schemes=("http", "https"), resolve_dns=True)`.
///
/// The default accepts HTTP and HTTPS and resolves a name. A URL is refused
/// when its host is this machine by name, an address that is loopback,
/// private, reserved or multicast (an IPv4 address in any of the forms the
/// client's parser reads, `127.1`, `0177.0.0.1` or `0x7f000001` among them),
/// or a name one of whose addresses is. Supply internationalized hostnames
/// as ASCII (Punycode), and IPv6 literals without a zone identifier. Paths
/// and queries may contain Unicode.
///
/// # What it does not guard
///
/// - **A name the client resolves again.** The function checks the answers
///   it gets and returns only the URL. The client then resolves the name
///   again, and may be given other addresses (DNS rebinding): nothing
///   returned lets a caller pin the connection to an address that was
///   checked. A caller that pins a connection resolves the name itself and
///   checks the address it dials by passing it as a URL's host
///   (`http://<address>/`, an IPv6 address in brackets): nothing else
///   public judges an address.
/// - **A client behind a proxy.** The proxy resolves the name, not the
///   client: the lookup made here says nothing of what the proxy reaches.
/// - **Redirects.** Only the URL given is checked. A client that follows
///   redirects leaves the guard after the first hop: check each hop, or do
///   not follow them.
/// - **A name, without resolution.** `resolve_dns: false` checks the URL as
///   it is written only. An address is judged. A name is refused in three
///   cases: it is one of the names of this machine the function always
///   refuses (`localhost`, `ip6-localhost` and `ip6-loopback`, in any case,
///   with or without trailing dots); the parser refuses the host, as said
///   below; or it ends in dots and, without them, spells an address that is
///   not public. Any other name is accepted unchecked, `sub.localhost` and
///   `localhost.localdomain` among them. It is for tests, and for a caller
///   that resolves a name and checks its addresses itself.
///
/// # Blocking
///
/// A name is resolved synchronously, by the system's resolver, with no
/// timeout of its own: the call blocks its thread until the resolver
/// answers, an async executor's thread included. Call it from a blocking
/// task.
///
/// # How a host is read
///
/// The host is read by the `url` crate. A host that parser reads as an
/// address is judged as the address the client dials, for a client that
/// reads the URL with the same crate. `reqwest` 0.13 does: its `Url` is
/// that crate's, and within the 2.x line cargo keeps one version of it in a
/// build. A client with another parser may read another host. This holds
/// for an address, not for a name: a name is resolved here, then again by
/// the client.
///
/// A host that ends in two dots or more is a name to the parser, whatever
/// stands before them. It is refused when, without its dots, it spells an
/// address that is not public: `127.0.01..` is refused as the address
/// `127.0.0.1`, as Python refuses it. Otherwise it stays the name the
/// client resolves: it is resolved here, without its dots, and every answer
/// checked.
///
/// The parser reads a host otherwise than Python does in four cases:
///
/// - a host whose last label is a number and that is no IPv4 address is
///   refused, where Python takes it for a name: `999.1.1.1`, `1.2.3.4.5`,
///   `08.1.1.1`, `example.123`, `example.0x10`;
/// - a hexadecimal part without a digit is read as zero, wherever it
///   stands, where Python takes the host for a name: `0x` is judged as the
///   address `0.0.0.0`, and `0xf.0x.0x.0x` as `15.0.0.0`, without a lookup;
/// - a label that starts with `xn--` and is no Punycode is refused, where
///   Python takes the host for a name: `xn--a.example`;
/// - a public address followed by two dots or more is a name, resolved as
///   said above, where Python reads the address and asks no resolver:
///   `1.2.3..`, `93.184.216.34..`.
///
/// A refusal says what Python's says, with three exceptions: a bracketed
/// host that is no IPv6 address and a pair of brackets in the user
/// information are refused in one wording each, where Python's depends on
/// the fault; and a character past ASCII in a port's text is written as it
/// is, where Python escapes one that is not printable.
///
/// ```
/// use roomkit_providers_http::{ValidatePublicUrlOptions, validate_public_url};
///
/// // No lookup here: an address is judged as it is written.
/// let options = ValidatePublicUrlOptions::default();
/// assert_eq!(
///     validate_public_url("https://93.184.216.34/hook", &options),
///     Ok("https://93.184.216.34/hook")
/// );
/// assert_eq!(
///     validate_public_url("http://127.1/", &options),
///     Err("URL host '127.1' resolves to loopback address 127.0.0.1".to_owned())
/// );
/// ```
///
/// # Errors
///
/// Returns Python's words for an invalid URL, a disallowed scheme or
/// address, or a hostname with no usable DNS answers.
pub fn validate_public_url<'u>(
    url: &'u str,
    options: &ValidatePublicUrlOptions<'_>,
) -> Result<&'u str, String> {
    validate(url, options, |host, port| {
        (host, port)
            .to_socket_addrs()
            .map(|results| results.map(|addr| addr.ip()).collect())
            .unwrap_or_default()
    })
}

/// [`validate_public_url`], a name's addresses given by `resolve`: every
/// address of the host at the port, none when it has none.
fn validate<'u>(
    original: &'u str,
    options: &ValidatePublicUrlOptions<'_>,
    resolve: impl FnOnce(&str, u16) -> Vec<IpAddr>,
) -> Result<&'u str, String> {
    let authority = Authority::read(original)?;
    let scheme = authority.scheme.to_ascii_lowercase();
    if !options.allowed_schemes.contains(&scheme.as_str()) {
        return Err(format!(
            "URL scheme must be one of {}, got {}",
            tuple(options.allowed_schemes),
            quoted(&scheme)
        ));
    }
    let port = authority.port()?;
    // The host as written names what a refusal says; a trailing dot names
    // the same host.
    let written = authority.hostname();
    let hostname = written.trim_end_matches('.');
    if LOCALHOST_NAMES.contains(&hostname) {
        return Err(format!(
            "URL must not point to localhost (host={})",
            quoted(&written)
        ));
    }
    let host = Host::read(original, &scheme, authority.rest)?;
    let Some(name) = judged_as_written(host, hostname)? else {
        return Ok(original);
    };
    if !options.resolve_dns {
        return Ok(original);
    }
    let default_port = if scheme == "https" { 443 } else { 80 };
    let port = port.filter(|port| *port != 0).unwrap_or(default_port);
    check_answers(hostname, resolve(&name, port))?;
    Ok(original)
}

/// Judges a host as the URL writes it: `None` for a public address, which
/// the client dials and no lookup stands behind, or the name to resolve.
///
/// A name that ends in dots may spell an address once they are dropped.
/// That address refuses the URL when it is not public, as Python refuses
/// the address it reads once it has dropped the dots. It never stands for
/// the name: the client resolves the name by itself, so the name is
/// resolved here too.
///
/// # Errors
///
/// Python's words for an address that is not public.
fn judged_as_written(host: Host, hostname: &str) -> Result<Option<String>, String> {
    match host {
        Host::Address(address) => addresses::check(hostname, address).map(|()| None),
        Host::Name { name, spelled } => {
            if let Some(address) = spelled {
                addresses::check(hostname, address)?;
            }
            Ok(Some(name))
        }
    }
}

/// Checks every address a name resolved to.
///
/// # Errors
///
/// Python's words for a name without an answer, or with an answer that is
/// not public.
fn check_answers(hostname: &str, answers: Vec<IpAddr>) -> Result<(), String> {
    if answers.is_empty() {
        return Err(format!(
            "URL host {} did not resolve to any address",
            quoted(hostname)
        ));
    }
    for address in answers {
        addresses::check(hostname, address)?;
    }
    Ok(())
}

#[cfg(test)]
mod python_public_urls;
#[cfg(test)]
mod tests;
