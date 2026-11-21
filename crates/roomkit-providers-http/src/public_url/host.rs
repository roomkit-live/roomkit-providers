//! The host of a URL as the client's parser reads it: the `url` crate, the
//! parser `reqwest` reads a URL with.

use std::net::IpAddr;

use url::Url;

use super::authority::NO_SCHEME_OR_HOST;

/// A URL's host, read by the client's parser.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Host {
    /// An address, which the client dials: an IPv4 address in its four
    /// parts whatever form it was written in, or an IPv6 address.
    Address(IpAddr),
    /// A name, which the client resolves.
    Name {
        /// The name in lower case, without its trailing dots.
        name: String,
        /// The address the parser reads the name as once its trailing dots
        /// are dropped, when dots were dropped and it reads one:
        /// `127.0.01..` spells `127.0.0.1`.
        spelled: Option<IpAddr>,
    },
}

impl Host {
    /// Reads the host of `original`, whose scheme is `scheme` and whose text
    /// after `://` is `rest`.
    ///
    /// A scheme other than HTTP's gets the same reading: its host is read as
    /// an HTTP URL's, where the parser would keep it as an opaque text.
    ///
    /// # Errors
    ///
    /// The parser's refusal, or Python's words for a URL without a host.
    pub(super) fn read(original: &str, scheme: &str, rest: &str) -> Result<Self, String> {
        let read = if matches!(scheme, "http" | "https") {
            parsed_host(original)?
        } else {
            parsed_host(&format!("http://{rest}"))?
        };
        if let Ok(address) = read.parse() {
            return Ok(Self::Address(address));
        }
        let name = read.trim_end_matches('.');
        if name.is_empty() {
            return Err(NO_SCHEME_OR_HOST.to_owned());
        }
        // The parser takes a host that ends in two dots or more for a name,
        // whatever stands before them.
        let spelled = if name.len() == read.len() {
            None
        } else {
            spelled_address(name)
        };
        Ok(Self::Name {
            name: name.to_owned(),
            spelled,
        })
    }
}

/// The address the parser reads `name` as, when it reads one.
fn spelled_address(name: &str) -> Option<IpAddr> {
    parsed_host(&format!("http://{name}/")).ok()?.parse().ok()
}

/// The host of `url` as the parser reads it, an IPv6 address without its
/// brackets.
fn parsed_host(url: &str) -> Result<String, String> {
    let url = Url::parse(url).map_err(|error| format!("Invalid URL: {error}"))?;
    let host = url.host_str().unwrap_or_default();
    let literal = host.strip_prefix('[').and_then(|h| h.strip_suffix(']'));
    Ok(literal.unwrap_or(host).to_owned())
}
