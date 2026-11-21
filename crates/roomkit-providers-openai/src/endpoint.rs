//! Whether a base URL is OpenAI's own endpoint, as Python's
//! `is_vendor_endpoint` compares them, and how a base URL may be shown.
//!
//! A URL is split here as Python's `urlsplit` splits it, by hand: the
//! comparison decides which defaults a configuration takes, so it follows
//! Python's reading of a URL rather than a URL parser's, which normalizes a
//! path and a host that Python compares as they are written.

/// OpenAI's own endpoint, which a configuration with no base URL calls.
pub(crate) const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

/// The host, the port and the path of [`OPENAI_BASE_URL`].
const OPENAI_HOST: &str = "api.openai.com";
const OPENAI_PORT: u32 = 443;
const OPENAI_PATH: &str = "/v1";

/// What a URL names, as far as an endpoint is compared and shown.
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
    /// is not ASCII: neither is compared nor shown.
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

    /// Whether the URL names OpenAI's own endpoint: its scheme and host,
    /// case aside, the scheme's default port written out or not, its path
    /// with trailing slashes or without, and no credentials.
    fn is_openai(&self) -> bool {
        let default_port = match self.port.as_deref() {
            None => true,
            Some(port) => Self::port_number(port) == Some(OPENAI_PORT),
        };
        self.scheme == "https"
            && !self.carries_credentials()
            && !self.bracketed
            && self.host.eq_ignore_ascii_case(OPENAI_HOST)
            && default_port
            && self.path.trim_end_matches('/') == OPENAI_PATH
    }

    /// The scheme, the host and the port, when each is made of the
    /// characters a scheme, a name or an address and a port are written
    /// with: never the user information, the path or the query.
    fn shown(&self) -> Option<String> {
        let named = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_');
        let addressed = |c: char| c.is_ascii_hexdigit() || matches!(c, ':' | '.');
        let host = if self.bracketed {
            Some(&self.host)
                .filter(|host| !host.is_empty() && host.chars().all(addressed))
                .map(|host| format!("[{host}]"))
        } else {
            Some(&self.host)
                .filter(|host| !host.is_empty() && host.chars().all(named))
                .map(|host| host.to_ascii_lowercase())
        }?;
        match self.port.as_deref() {
            None => Some(format!("{}://{host}", self.scheme)),
            Some(port) if Self::port_number(port).is_some() => {
                Some(format!("{}://{host}:{port}", self.scheme))
            }
            Some(_) => None,
        }
    }
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

/// Whether `base_url` is OpenAI's own endpoint: none given, or
/// [`OPENAI_BASE_URL`] however it is written (Python's
/// `is_vendor_endpoint(base_url, OPENAI_BASE_URL)`).
pub(crate) fn is_openai_endpoint(base_url: Option<&str>) -> bool {
    base_url.is_none_or(|url| Endpoint::split(url).is_some_and(|endpoint| endpoint.is_openai()))
}

/// `url` as it may be shown: its scheme, host and port, never its user
/// information, path or query, which may carry credentials.
pub(crate) fn shown_url(url: &str) -> String {
    Endpoint::split(url)
        .and_then(|endpoint| endpoint.shown())
        .unwrap_or_else(|| "(not a URL)".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_is_shown_without_its_user_information_path_or_query() {
        let shown = |url: &str| shown_url(url);
        assert_eq!(shown(OPENAI_BASE_URL), "https://api.openai.com");
        assert_eq!(
            shown(
                "HTTPS://user:placeholder-not-a-key@LLM.Example:8443/v1/placeholder?key=placeholder#f"
            ),
            "https://llm.example:8443"
        );
        assert_eq!(shown("http://[::1]:8000/v1"), "http://[::1]:8000");
        assert_eq!(shown("http://localhost:11434/v1"), "http://localhost:11434");
        for hidden in [
            "placeholder-not-a-key",
            "llm.example/v1",
            "https://",
            "https://llm.example:port/v1",
            "https://user@placeholder not a key/v1",
            "https://[placeholder]/v1",
        ] {
            assert_eq!(shown(hidden), "(not a URL)", "{hidden}");
        }
    }
}
