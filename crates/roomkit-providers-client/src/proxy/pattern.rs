//! What a proxy variable holds, read as the URL class of Python's client
//! reads it (httpx2's `URL` and `URLPattern`): the URL of a proxy, and the
//! pattern of the endpoints a rule applies to.
//!
//! The reading is Python's, quirks included, so that the same environment
//! sends the same requests through the same proxies: a pattern's own port is
//! dropped only under a scheme written in lower case, a star stands for
//! other names at the start of a host alone, an address is one host and
//! never a range.

use std::cmp::Reverse;
use std::net::{Ipv4Addr, Ipv6Addr};

/// The most characters of a URL Python's client reads.
const LONGEST: usize = 65_536;

/// A text of a proxy variable that Python's client refuses, or that this
/// one does not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Unread;

/// A host as written in a proxy variable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Host {
    /// None is written.
    Absent,
    /// A name or an IPv4 address, in lower case.
    Name(String),
    /// An IPv6 address, written in brackets, and the length it was written
    /// in.
    V6(Ipv6Addr, usize),
    /// A name that holds a character past ASCII, as written.
    International(String),
}

/// What Python's URL class reads of a text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Read<'t> {
    /// The scheme, in lower case; empty when the text names none.
    pub(super) scheme: String,
    /// What stands before the last `@` of the authority, as written.
    pub(super) user_information: &'t str,
    pub(super) host: Host,
    /// The port, unless it is the scheme's own.
    pub(super) port: Option<u32>,
}

/// The scheme `text` starts with, as written, and what follows its colon:
/// a letter, then letters, digits, `+`, `.` and `-`, up to a colon. A text
/// that starts with a colon has an empty scheme; any other has none, and is
/// read whole.
fn scheme_of(text: &str) -> (&str, &str) {
    let in_a_scheme = |byte: &u8| byte.is_ascii_alphanumeric() || b"+.-".contains(byte);
    let named = text.bytes().next().is_some_and(|b| b.is_ascii_alphabetic());
    let length = if named {
        text.bytes().take_while(in_a_scheme).count()
    } else {
        0
    };
    let (scheme, rest) = text.split_at_checked(length).unwrap_or(("", text));
    match rest.strip_prefix(':') {
        Some(after) => (scheme, after),
        None => ("", text),
    }
}

/// The host and the port of an authority, after its user information: a
/// host in brackets ends at the last bracket, any other at the first colon.
fn host_and_port(text: &str) -> (&str, &str) {
    let bracketed = text.starts_with('[').then(|| text.rsplit_once(']'));
    if let Some((_, after)) = bracketed.flatten() {
        let host = text.len().saturating_sub(after.len());
        let (host, after) = text.split_at_checked(host).unwrap_or((text, ""));
        return (host, after.strip_prefix(':').unwrap_or(after));
    }
    text.split_once(':').unwrap_or((text, ""))
}

/// A host as Python's `encode_host` reads it: four numbers between dots
/// must be an IPv4 address, a host in brackets an IPv6 one.
fn host_of(written: &str) -> Result<Host, Unread> {
    if written.is_empty() {
        return Ok(Host::Absent);
    }
    let numbers: Vec<&str> = written.split('.').collect();
    let number = |part: &&str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    if numbers.len() == 4 && numbers.iter().all(number) {
        let address: Ipv4Addr = written.parse().map_err(|_| Unread)?;
        return Ok(Host::Name(address.to_string()));
    }
    let inside = written.strip_prefix('[').and_then(|w| w.strip_suffix(']'));
    if let Some(inside) = inside {
        let address = inside.parse().map_err(|_| Unread)?;
        return Ok(Host::V6(address, inside.len()));
    }
    if written.is_ascii() {
        Ok(Host::Name(written.to_ascii_lowercase()))
    } else {
        Ok(Host::International(written.to_owned()))
    }
}

/// A port as Python's `normalize_port` reads it, under the scheme as it was
/// written: none when it is empty or that scheme's own. Python looks the
/// scheme up as written, so `HTTPS://host:443` keeps its port. A port
/// written otherwise than in digits is not read here.
fn port_of(written: &str, scheme: &str) -> Result<Option<u32>, Unread> {
    if written.is_empty() {
        return Ok(None);
    }
    if !written.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Unread);
    }
    let digits = written.trim_start_matches('0');
    // A number no port reaches is kept as one no endpoint has.
    let port = match digits.len() {
        0 => 0,
        1..=9 => digits.parse().map_err(|_| Unread)?,
        _ => u32::MAX,
    };
    let own = match scheme {
        "ftp" => Some(21),
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        _ => None,
    };
    Ok(Some(port).filter(|port| Some(*port) != own))
}

/// `text` as Python's URL class reads it.
///
/// # Errors
///
/// A text Python's client refuses when it is built: one that holds an
/// ASCII control character, an address that is none, a port that is no
/// number, a path that cannot follow what stands before it. And a port
/// written otherwise than in digits, which this client does not read.
pub(super) fn read(text: &str) -> Result<Read<'_>, Unread> {
    let printable = |c: char| !c.is_ascii() || (' '..='~').contains(&c);
    if text.chars().count() > LONGEST || !text.chars().all(printable) {
        return Err(Unread);
    }
    let (scheme, rest) = scheme_of(text);
    let (authority, path) = match rest.strip_prefix("//") {
        Some(after) => {
            let end = after.find(['/', '?', '#']).unwrap_or(after.len());
            after.split_at_checked(end).unwrap_or((after, ""))
        }
        None => ("", rest),
    };
    let (user_information, after) = authority.rsplit_once('@').unwrap_or(("", authority));
    let (host, port) = host_and_port(after);
    let host = host_of(host)?;
    let port = port_of(port, scheme)?;
    let has_authority = !user_information.is_empty() || host != Host::Absent || port.is_some();
    // A text that names neither a scheme nor an authority is a relative
    // reference, whose path starts with neither `//` nor a colon.
    let relative = scheme.is_empty() && !has_authority;
    if relative && (path.starts_with("//") || path.starts_with(':')) {
        return Err(Unread);
    }
    Ok(Read {
        scheme: scheme.to_ascii_lowercase(),
        user_information,
        host,
        port,
    })
}

/// The hosts a pattern applies to.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Hosts {
    /// Every host.
    Every,
    /// The host of that name, or of that IPv4 address.
    Named(String),
    /// The host of that IPv6 address.
    Address(Ipv6Addr),
    /// The hosts under a name: `*.name`.
    Under(String),
    /// A name and the hosts under it: `*name`.
    AtOrUnder(String),
}

/// Where a pattern stands among the others: one with a port first, then
/// the longest host, then the longest scheme, as Python sorts them.
pub(super) type Rank = (bool, Reverse<usize>, Reverse<usize>);

/// The endpoints a rule applies to: Python's `URLPattern`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Pattern {
    /// The scheme, when the pattern names one other than `all`.
    scheme: Option<String>,
    hosts: Hosts,
    port: Option<u32>,
    pub(super) rank: Rank,
}

/// Whether `name` is under `domain`: something, a dot, then `domain`.
fn under(name: &str, domain: &str) -> bool {
    let before = name.strip_suffix(domain);
    let label = before.and_then(|before| before.strip_suffix('.'));
    label.is_some_and(|label| !label.is_empty())
}

impl Pattern {
    /// The pattern `written` is, a URL whose scheme may be `all` and whose
    /// host may start with a star.
    ///
    /// # Errors
    ///
    /// A text Python's client refuses when it is built (see [`read`]), a
    /// name in Punycode beside a star among them. And a name that holds a
    /// character past ASCII, which this client does not read.
    pub(super) fn of(written: &str) -> Result<Self, Unread> {
        let read = read(written)?;
        let scheme = Some(read.scheme).filter(|scheme| !scheme.is_empty() && scheme != "all");
        let (hosts, length) = match read.host {
            Host::Absent => (Hosts::Every, 0),
            Host::V6(address, length) => (Hosts::Address(address), length),
            Host::International(_) => return Err(Unread),
            Host::Name(name) => {
                if name.contains("xn--") && name.contains('*') {
                    return Err(Unread);
                }
                let length = name.len();
                if name == "*" {
                    (Hosts::Every, 0)
                } else if let Some(domain) = name.strip_prefix("*.") {
                    (Hosts::Under(domain.to_owned()), length)
                } else if let Some(domain) = name.strip_prefix('*') {
                    (Hosts::AtOrUnder(domain.to_owned()), length)
                } else {
                    (Hosts::Named(name), length)
                }
            }
        };
        let scheme_length = scheme.as_ref().map_or(0, String::len);
        Ok(Self {
            scheme,
            hosts,
            port: read.port,
            rank: (read.port.is_none(), Reverse(length), Reverse(scheme_length)),
        })
    }

    /// Whether the pattern applies to `url`, an endpoint: its scheme, its
    /// host as the HTTP client reads it, and its port unless it is its
    /// scheme's own.
    pub(super) fn applies_to(&self, url: &reqwest::Url) -> bool {
        let scheme = |scheme: &String| scheme == url.scheme();
        let port = |port: u32| url.port().map(u32::from) == Some(port);
        self.scheme.as_ref().is_none_or(scheme)
            && self.port.is_none_or(port)
            && self.applies_to_host(url)
    }

    /// Whether the pattern applies to the host of `url`, as the HTTP client
    /// reads it: a name in lower case, or an address in the one form the
    /// client writes it in.
    fn applies_to_host(&self, url: &reqwest::Url) -> bool {
        let (name, address) = match url.host() {
            Some(url::Host::Domain(name)) => (name.to_owned(), None),
            Some(url::Host::Ipv4(address)) => (address.to_string(), None),
            Some(url::Host::Ipv6(address)) => (address.to_string(), Some(address)),
            None => (String::new(), None),
        };
        match &self.hosts {
            Hosts::Every => true,
            Hosts::Named(named) => *named == name,
            Hosts::Address(named) => address == Some(*named),
            Hosts::Under(domain) => under(&name, domain),
            Hosts::AtOrUnder(domain) => name == *domain || under(&name, domain),
        }
    }
}
