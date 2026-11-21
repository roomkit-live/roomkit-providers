//! The authority of a URL as it is written, read before any parser
//! normalizes it: Python's `_check_public_authority`, then the host and the
//! port its `urlparse` reads.

use std::net::Ipv6Addr;

use super::words::quoted;

/// What the refusal of a URL without a scheme or without a host says.
pub(super) const NO_SCHEME_OR_HOST: &str = "URL must have a scheme and host";

/// What the refusal of a bracket outside an IPv6 literal says, in the words
/// of Python's `urlsplit`.
const STRAY_BRACKET: &str = "Invalid IPv6 URL";

/// The characters past ASCII that Unicode's compatibility forms (NFKC) turn
/// into a text holding `/`, `?`, `#`, `@` or `:`, as of Unicode 15.1: the
/// wide and small forms of those marks, and signs such as the "account of"
/// sign. Python's `urlsplit` refuses an authority that holds one, since a
/// reader that normalizes it would split it elsewhere.
pub(super) const DELIMITER_LOOKALIKES: [char; 19] = [
    '\u{2047}', '\u{2048}', '\u{2049}', '\u{2100}', '\u{2101}', '\u{2105}', '\u{2106}', '\u{2a74}',
    '\u{fe13}', '\u{fe16}', '\u{fe55}', '\u{fe56}', '\u{fe5f}', '\u{fe6b}', '\u{ff03}', '\u{ff0f}',
    '\u{ff1a}', '\u{ff1f}', '\u{ff20}',
];

/// A URL's scheme and authority, as written.
#[derive(Debug)]
pub(super) struct Authority<'u> {
    /// The scheme, as written before `://`.
    pub(super) scheme: &'u str,
    /// What follows `://`: the authority, then the path, the query and the
    /// fragment.
    pub(super) rest: &'u str,
    /// The host, without the brackets of an IPv6 literal.
    host: &'u str,
    /// The text after the host's `:`, when there is some.
    port: Option<&'u str>,
}

impl<'u> Authority<'u> {
    /// Reads the scheme and the authority of `original`.
    ///
    /// # Errors
    ///
    /// Python's words for a URL without a scheme or a host, for an authority
    /// that holds a backslash, a space, a control character or two `@`, for
    /// a bracketed host that is no IPv6 address, for a host that is no
    /// ASCII name nor address, for a bracket beside a name, and for a
    /// character that normalizes to a delimiter.
    pub(super) fn read(original: &'u str) -> Result<Self, String> {
        let invalid = || NO_SCHEME_OR_HOST.to_owned();
        let url = original.trim_start_matches([' ', '\t', '\r', '\n']);
        let (scheme, rest) = url.split_once("://").ok_or_else(invalid)?;
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if !is_scheme(scheme) || authority.is_empty() {
            return Err(invalid());
        }
        if authority.contains('\\')
            || authority.bytes().any(|b| b <= b' ' || b == 0x7f)
            || authority.matches('@').count() > 1
        {
            return Err("Invalid URL authority".to_owned());
        }
        let (userinfo, host_port) = authority.rsplit_once('@').unwrap_or(("", authority));
        let (host, port) = match host_port.strip_prefix('[') {
            Some(bracketed) => ipv6_host(bracketed)?,
            None => named_host(userinfo, host_port)?,
        };
        if authority.contains(DELIMITER_LOOKALIKES) {
            // Python writes the authority between quotes, as it is.
            return Err(format!(
                "netloc '{authority}' contains invalid characters under NFKC normalization"
            ));
        }
        Ok(Self {
            scheme,
            rest,
            host,
            port: port.filter(|text| !text.is_empty()),
        })
    }

    /// The host in lower case: Python's `hostname`.
    pub(super) fn hostname(&self) -> String {
        self.host.to_ascii_lowercase()
    }

    /// The port, `None` when the URL states none.
    ///
    /// # Errors
    ///
    /// The words of Python's `urllib` for a port that is no number, or
    /// past 65535.
    pub(super) fn port(&self) -> Result<Option<u16>, String> {
        let Some(text) = self.port else {
            return Ok(None);
        };
        if !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(format!(
                "Port could not be cast to integer value as {}",
                quoted(text)
            ));
        }
        let number = text.bytes().fold(0_u32, |number, digit| {
            number
                .saturating_mul(10)
                .saturating_add(u32::from(digit - b'0'))
        });
        u16::try_from(number)
            .map(Some)
            .map_err(|_| "Port out of range 0-65535".to_owned())
    }
}

/// Whether `scheme` is written as a scheme: a letter, then letters, digits,
/// `+`, `-` and `.`.
fn is_scheme(scheme: &str) -> bool {
    let mut letters = scheme.bytes();
    letters
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic())
        && letters.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

/// The host and the port's text of `bracketed`, what follows the `[` of an
/// IPv6 literal.
fn ipv6_host(bracketed: &str) -> Result<(&str, Option<&str>), String> {
    let invalid = || "Invalid IPv6 host or port in URL authority".to_owned();
    let (literal, suffix) = bracketed.split_once(']').ok_or_else(invalid)?;
    let port = match suffix.strip_prefix(':') {
        Some(port) if port.bytes().all(|byte| byte.is_ascii_digit()) => Some(port),
        None if suffix.is_empty() => None,
        Some(_) | None => return Err(invalid()),
    };
    // A zone identifier is no part of an address a URL names.
    literal.parse::<Ipv6Addr>().map_err(|_| invalid())?;
    Ok((literal, port))
}

/// The host and the port's text of `host_port`, a name or an IPv4 address
/// with the port that may follow it, after the user information `userinfo`.
fn named_host<'u>(
    userinfo: &str,
    host_port: &'u str,
) -> Result<(&'u str, Option<&'u str>), String> {
    let (host, port) = match host_port.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (host_port, None),
    };
    if host.trim_end_matches('.').is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        return Err("URL host must be an ASCII DNS name or IP address".to_owned());
    }
    // Python's `urlsplit` takes a bracket for the start of an IPv6 literal
    // wherever it stands, and refuses one beside a host that is none.
    let bracket = |text: &str| text.contains(['[', ']']);
    if bracket(userinfo) || port.is_some_and(bracket) {
        return Err(STRAY_BRACKET.to_owned());
    }
    Ok((host, port))
}
