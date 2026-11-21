//! Check an outbound HTTP destination before handing it to a transport.

mod addresses;
mod authority;
mod words;

use std::net::{IpAddr, ToSocketAddrs};

use reqwest::Url;

use self::authority::{Authority, NO_SCHEME_OR_HOST};
use self::words::{quoted, tuple};

/// The names of this machine a URL may not name.
const LOCALHOST_NAMES: [&str; 3] = ["localhost", "ip6-localhost", "ip6-loopback"];

/// The keyword arguments of Python's `validate_public_url`: `allowed_schemes=("http",
/// "https")` and `resolve_dns=True`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidatePublicUrlOptions<'a> {
    /// Schemes accepted by this caller, in lower case.
    pub allowed_schemes: &'a [&'a str],
    /// Resolve a hostname and inspect every address it returns.
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
/// The default accepts HTTP and HTTPS and performs synchronous DNS
/// resolution. A URL is refused when its host is this machine by name, an
/// address that is loopback, private, reserved or multicast (an IPv4 address
/// in any of the forms a resolver reads, `127.1` or `0x7f000001` among them),
/// or a name one of whose addresses is. Supply internationalized hostnames
/// as ASCII (Punycode), and IPv6 literals without a zone identifier. Paths
/// and queries may contain Unicode.
///
/// The host is read by the parser the HTTP client of this repository dials
/// with, so that the address checked is the address dialed. Where that
/// parser reads a host otherwise than Python does, the URL is refused, and
/// Python takes it for a name:
///
/// - a host whose last label is a number and that is no IPv4 address:
///   `999.1.1.1`, `1.2.3.4.5`, `08.1.1.1`, `example.123`, `example.0x10`;
/// - `0x`, read as the address `0.0.0.0`;
/// - a label that starts with `xn--` and is no Punycode: `xn--a.example`.
///
/// A refusal says what Python's says, with three exceptions: a bracketed
/// host that is no IPv6 address and a pair of brackets in the user
/// information are refused in one wording each, where Python's depends on
/// the fault; and a character past ASCII in a port's text is written as it
/// is, where Python escapes one that is not printable.
///
/// Validation observes the answers at the time of the call. A transport that
/// needs to pin the connection must dial an address it has checked.
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
    let host = dialed_host(original, &scheme, authority.rest)?;
    if let Ok(address) = host.parse::<IpAddr>() {
        addresses::check(hostname, address)?;
        return Ok(original);
    }
    if !options.resolve_dns {
        return Ok(original);
    }
    let default_port = if scheme == "https" { 443 } else { 80 };
    let port = port.filter(|port| *port != 0).unwrap_or(default_port);
    let addresses = resolve(&host, port);
    if addresses.is_empty() {
        return Err(format!(
            "URL host {} did not resolve to any address",
            quoted(hostname)
        ));
    }
    for address in addresses {
        addresses::check(hostname, address)?;
    }
    Ok(original)
}

/// The host the client dials for the URL, as the client's own parser reads
/// it: in lower case, an IPv4 address in its four parts whatever form it was
/// written in, an IPv6 address without its brackets, no trailing dot.
///
/// A scheme other than HTTP's gets the same reading: its host is read as an
/// HTTP URL's, where the parser would keep it as an opaque text.
fn dialed_host(original: &str, scheme: &str, rest: &str) -> Result<String, String> {
    let url = if matches!(scheme, "http" | "https") {
        Url::parse(original)
    } else {
        Url::parse(&format!("http://{rest}"))
    }
    .map_err(|error| format!("Invalid URL: {error}"))?;
    let host = url.host_str().unwrap_or_default().trim_end_matches('.');
    if host.is_empty() {
        return Err(NO_SCHEME_OR_HOST.to_owned());
    }
    let literal = host.strip_prefix('[').and_then(|h| h.strip_suffix(']'));
    Ok(literal.unwrap_or(host).to_owned())
}

#[cfg(test)]
mod python_public_urls;
#[cfg(test)]
mod tests;
