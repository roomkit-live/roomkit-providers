//! Where a provider sends: the URL of a path of its API under a base URL,
//! with the base URLs refused when a provider is built; whether a base URL
//! is the vendor's own endpoint, as Python's `is_vendor_endpoint` compares
//! them; and how a base URL may be shown.
//!
//! A URL is compared as Python's `urlsplit` splits it, by hand: the
//! comparison decides which defaults a configuration takes, so it follows
//! Python's reading of a URL rather than a URL parser's, which normalizes a
//! path and a host that Python compares as they are written.

use crate::failure::Failure;

/// Compiles only while the URL the client dials is the `url` crate's, the
/// one `roomkit-providers-http` validates a destination with: a URL that
/// was checked there is read the same way here.
const fn the_client_s_url(url: reqwest::Url) -> url::Url {
    url
}

/// The URL of `path` under `base_url`, which the failure of a refused one
/// does not repeat: it may carry credentials.
///
/// A base URL is refused where Python's client and this one would not send
/// the same request: one that holds white space or a control character
/// (Python refuses a control character, and writes a space into the path),
/// one that holds a backslash (which ends the host for this client and not
/// for Python's), one whose scheme is not followed by `//` and a host
/// (Python sends such a request to no host), one whose path holds a dot
/// segment, written or percent-encoded (this client takes it out of the
/// path, Python's sends it), and one whose user information is no UTF-8
/// once decoded (Python sends replaced characters, this client none or
/// half of them).
///
/// The path follows the base URL as Python's client writes it: after the
/// one slash that ends the base URL, when one does.
///
/// # Errors
///
/// The failure of a provider that cannot be built, for a base URL that is
/// no `http` or `https` URL, that has a query or a fragment, which the
/// path cannot follow, or that the two clients read otherwise.
pub fn joined(base_url: &str, path: &str) -> Result<reqwest::Url, Failure> {
    let not_http = || Failure::unbuilt("its base URL is not an http or https URL");
    if base_url
        .chars()
        .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(Failure::unbuilt(
            "its base URL holds white space or a control character",
        ));
    }
    if base_url.contains('\\') {
        return Err(Failure::unbuilt("its base URL holds a backslash"));
    }
    let lowered = base_url.to_ascii_lowercase();
    let after_the_scheme = lowered.strip_prefix("https://");
    let after_the_scheme = after_the_scheme.or_else(|| lowered.strip_prefix("http://"));
    if after_the_scheme.ok_or_else(not_http)?.starts_with('/') {
        return Err(Failure::unbuilt("its base URL names no host"));
    }
    let base = reqwest::Url::parse(base_url).map_err(|_| not_http())?;
    if base.query().is_some() || base.fragment().is_some() {
        return Err(Failure::unbuilt(
            "its base URL has a query or a fragment, which the path of the API cannot follow",
        ));
    }
    if let Some(reason) = read_otherwise(&lowered, &base) {
        return Err(Failure::unbuilt(reason));
    }
    let joined = format!("{}{path}", base_url.strip_suffix('/').unwrap_or(base_url));
    let url = reqwest::Url::parse(&joined).map_err(|_| not_http())?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err(not_http()),
    }
}

/// Why Python's client would not send the request this one sends for
/// `base`, a base URL written `lowered` in lower case, when it would not:
/// its path holds a dot segment, written or percent-encoded, or its user
/// information is no UTF-8 once decoded.
fn read_otherwise(lowered: &str, base: &reqwest::Url) -> Option<&'static str> {
    let written = lowered.split_once("://").map_or("", |(_, rest)| rest);
    let segments = written.split_once('/').map_or("", |(_, path)| path);
    let is_dot = |segment: &str| matches!(segment.replace("%2e", ".").as_str(), "." | "..");
    if segments.split('/').any(is_dot) {
        return Some("its base URL holds a dot segment");
    }
    let reads = |part: &str| String::from_utf8(percent_decoded_bytes(part)).is_ok();
    if !reads(base.username()) || !base.password().is_none_or(reads) {
        return Some("its base URL holds user information that is no UTF-8");
    }
    None
}

/// Whether `url` holds user information, which the client sends as Basic
/// credentials: as with Python's client, they take the place of an
/// `Authorization` a provider or its configuration would write.
#[must_use]
pub fn has_user_information(url: &reqwest::Url) -> bool {
    !url.username().is_empty() || url.password().is_some()
}

/// The bytes of `text` with each `%XX` read as the byte it writes.
fn percent_decoded_bytes(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((first, after)) = rest.split_first() {
        let hex = after.get(..2).and_then(|hex| std::str::from_utf8(hex).ok());
        let byte = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(byte) = byte.filter(|_| *first == b'%') {
            bytes.push(byte);
            rest = after.get(2..).unwrap_or_default();
        } else {
            bytes.push(*first);
            rest = after;
        }
    }
    bytes
}

/// `text` with each `%XX` read as the byte it writes.
pub(crate) fn percent_decoded(text: &str) -> String {
    String::from_utf8_lossy(&percent_decoded_bytes(text)).into_owned()
}

/// What a URL names, as far as an endpoint is compared.
struct Endpoint {
    /// The scheme, in lower case.
    scheme: String,
    /// What stands before the last `@` of the authority, when it has one.
    user_information: Option<String>,
    /// The host as written, without the brackets of an IPv6 address.
    host: String,
    /// Whether the host was written in brackets.
    bracketed: bool,
    /// The text after the host's `:`, when there is some.
    port: Option<String>,
    /// The path, without the query and the fragment.
    path: String,
}

impl Endpoint {
    /// Splits `url`, or `None` for a text with no scheme or whose authority
    /// is not ASCII: neither is compared.
    fn split(url: &str) -> Option<Self> {
        let url = cleaned(url);
        let (scheme, rest) = url.split_once(':')?;
        if !is_scheme(scheme) {
            return None;
        }
        let rest = rest.strip_prefix("//").unwrap_or_default();
        let (authority, after) = match rest.find(['/', '?', '#']) {
            Some(at) => rest.split_at(at),
            None => (rest, ""),
        };
        if !authority.is_ascii() {
            return None;
        }
        let path = after.split('#').next().unwrap_or_default();
        let path = path.split('?').next().unwrap_or_default();
        let (user_information, host_and_port) = match authority.rsplit_once('@') {
            Some((user, host)) => (Some(user.to_owned()), host),
            None => (None, authority),
        };
        let (host, bracketed, port) = host_and_port_of(host_and_port);
        Some(Self {
            scheme: scheme.to_ascii_lowercase(),
            user_information,
            host: host.to_owned(),
            bracketed,
            port: Some(port)
                .filter(|port| !port.is_empty())
                .map(str::to_owned),
            path: path.to_owned(),
        })
    }

    /// Whether the URL carries a user name or a password: Python reads an
    /// empty pair (`@`, `:@`) as none.
    fn carries_credentials(&self) -> bool {
        self.user_information
            .as_deref()
            .is_some_and(|user| !matches!(user, "" | ":"))
    }

    /// The port as a number, `None` for a text that is no number: Python's
    /// `port.isdigit() and port.isascii()`, then `int(port)`.
    fn port_number(port: &str) -> Option<u32> {
        if !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        // A number past 32 bits is past any port.
        Some(port.parse().unwrap_or(u32::MAX))
    }

    /// The port a scheme dials when a URL writes none (Python's
    /// `_DEFAULT_PORTS`).
    fn default_port(scheme: &str) -> Option<u32> {
        match scheme {
            "http" | "ws" => Some(80),
            "https" | "wss" => Some(443),
            _ => None,
        }
    }

    /// What the URL names as an endpoint (Python's `_comparable`): its
    /// scheme and host, case aside, its port, none for the scheme's default
    /// written out or not, and its path with trailing slashes or without.
    /// `None` for a URL that carries credentials or whose port is no
    /// number: it names no vendor's endpoint.
    fn named(&self) -> Option<Named> {
        if self.carries_credentials() {
            return None;
        }
        let port = match self.port.as_deref() {
            None => None,
            Some(port) => {
                let port = Self::port_number(port)?;
                Some(port).filter(|port| Some(*port) != Self::default_port(&self.scheme))
            }
        };
        Some(Named {
            scheme: self.scheme.clone(),
            host: self.host.to_ascii_lowercase(),
            bracketed: self.bracketed,
            port,
            path: self.path.trim_end_matches('/').to_owned(),
        })
    }
}

/// What a URL names as an endpoint: two URLs name the same endpoint when
/// they name the same of each.
#[derive(PartialEq, Eq)]
struct Named {
    scheme: String,
    host: String,
    bracketed: bool,
    port: Option<u32>,
    path: String,
}

/// `url` as `urlsplit` reads it once Python's `_comparable` stripped it:
/// without the spaces around it, the control characters before it, nor any
/// tab or line break.
fn cleaned(url: &str) -> String {
    // What Python's `str.strip()` strips: the white space Rust knows, and
    // the four separators of information, U+001C to U+001F.
    let space = |c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c);
    url.trim_matches(space)
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect()
}

/// Whether `text` is a URL's scheme: an ASCII letter, then letters, digits,
/// `+`, `-` and `.`.
fn is_scheme(text: &str) -> bool {
    let signs = |c: char| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.');
    text.starts_with(|c: char| c.is_ascii_alphabetic()) && text.chars().all(signs)
}

/// The host of an authority's `host:port`, whether it was written in
/// brackets, and the text of its port, as `urlsplit` tells them apart.
fn host_and_port_of(text: &str) -> (&str, bool, &str) {
    let Some((_, bracketed)) = text.split_once('[') else {
        let (host, port) = text.split_once(':').unwrap_or((text, ""));
        return (host, false, port);
    };
    let (host, after) = bracketed.split_once(']').unwrap_or((bracketed, ""));
    let port = after.split_once(':').map_or("", |(_, port)| port);
    (host, true, port)
}

/// Whether `base_url` is the vendor's own endpoint: none given, or one of
/// its `official` URLs however it is written, its scheme's and its host's
/// case, its scheme's default port written out or not, its trailing
/// slashes (Python's `is_vendor_endpoint(base_url, *official)`). A URL
/// that carries credentials is another endpoint: the vendor's takes none
/// there.
#[must_use]
pub fn is_vendor_endpoint(base_url: Option<&str>, official: &[&str]) -> bool {
    let named = |url: &str| Endpoint::split(url).and_then(|endpoint| endpoint.named());
    base_url.is_none_or(|url| {
        named(url).is_some_and(|given| {
            official
                .iter()
                .any(|url| named(url).as_ref() == Some(&given))
        })
    })
}

/// Whether a provider talks to its vendor's own endpoint, whose models and
/// rules it knows: its `base_url` is one of the vendor's `official` URLs as
/// Python compares them ([`is_vendor_endpoint`]), and `dialed`, the URL
/// the client dials for `path` under it, is the one the client dials for
/// `path` under that official URL, as the client reads both.
#[must_use]
pub fn talks_to_vendor(
    base_url: Option<&str>,
    official: &[&str],
    dialed: &reqwest::Url,
    path: &str,
) -> bool {
    let dialed = the_client_s_url(dialed.clone());
    let same = |official: url::Url| {
        official.scheme() == dialed.scheme()
            && official.host_str() == dialed.host_str()
            && official.port_or_known_default() == dialed.port_or_known_default()
            && official.path() == dialed.path()
    };
    let named = official.iter().filter_map(|url| joined(url, path).ok());
    is_vendor_endpoint(base_url, official)
        && dialed.username().is_empty()
        && dialed.password().is_none()
        && named.map(the_client_s_url).any(same)
}

/// `url` as it may be shown: the origin the HTTP client dials for it, its
/// scheme, host and port as the client's own URL parser reads them, never
/// its user information, path or query, which may carry credentials.
#[must_use]
pub fn shown_url(url: &str) -> String {
    let Ok(read) = url::Url::parse(url) else {
        return "(not a URL)".to_owned();
    };
    match (read.host_str(), read.port()) {
        (Some(host), Some(port)) => format!("{}://{host}:{port}", read.scheme()),
        (Some(host), None) => format!("{}://{host}", read.scheme()),
        (None, _) => "(not a URL)".to_owned(),
    }
}

/// Writes `address` as the host and the port of `url`, over plain HTTP,
/// its path and its user information as they were: a local fake of the
/// endpoint. `false` when the URL could not take it.
#[cfg(any(test, feature = "replay", feature = "harness"))]
#[must_use]
pub fn moved_to(url: &mut reqwest::Url, address: std::net::SocketAddr) -> bool {
    url.set_scheme("http").is_ok()
        && url.set_ip_host(address.ip()).is_ok()
        && url.set_port(Some(address.port())).is_ok()
}

#[cfg(test)]
mod tests;
