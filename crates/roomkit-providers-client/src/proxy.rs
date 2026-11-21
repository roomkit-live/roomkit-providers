//! The proxies a client sends through: what the environment names when the
//! client is built, read as the client of `RoomKit`'s Python providers
//! reads it, so that a host keeps the network it had.
//!
//! The variables are `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` and
//! `NO_PROXY`, in any case, as Python's `urllib` and `httpx2` read them:
//!
//! - a name that ends in lower case (`http_proxy`) wins over the others,
//!   and set to nothing it unsets them;
//! - under `REQUEST_METHOD`, the mark of a CGI program, `HTTP_PROXY` is
//!   dropped unless its name ends in lower case, since a request's `Proxy`
//!   header would set it;
//! - a proxy written without a scheme is an `http` one; `ALL_PROXY` serves
//!   the scheme that has no proxy of its own;
//! - an entry of `NO_PROXY` exempts a name and what is under it, the hosts
//!   under a name when it starts with a dot, one address, `localhost`
//!   alone, each on one port when it names one; `*` exempts everything, and
//!   an entry written as a URL is a pattern of Python's client.
//!
//! What Python's client refuses when it is built is refused when this one
//! is: a proxy that is neither `http` nor `https`, a text that does not
//! read. Nothing of a variable is repeated in the refusal: a proxy's URL
//! may carry credentials.

use std::collections::HashMap;
use std::fmt;

use self::pattern::{Host, Pattern, Unread};
use crate::failure::Failure;

mod pattern;

/// What marks a CGI program, whose `HTTP_PROXY` a request could set.
const CGI: &str = "REQUEST_METHOD";

/// The schemes a proxy is named for, in the order Python's client mounts
/// them.
const SCHEMES: [&str; 3] = ["http", "https", "all"];

/// A rule of the environment: the endpoints it applies to, and the proxy
/// they are reached through, or none when they are reached directly.
#[derive(Clone)]
struct Rule {
    pattern: Pattern,
    proxy: Option<reqwest::Url>,
}

/// The proxies a client sends through, as the environment named them when
/// the client was built.
#[derive(Clone, Default)]
pub(crate) struct Proxies {
    /// The rules, the first that applies to an endpoint deciding for it.
    rules: Vec<Rule>,
}

impl fmt::Debug for Proxies {
    /// How many rules, and how many of them name a proxy: never a proxy's
    /// URL, which may carry credentials.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Proxies")
            .field("rules", &self.rules.len())
            .field("proxies", &self.urls().count())
            .finish()
    }
}

/// What the environment's variables name, by the scheme each is for, as
/// Python's `getproxies_environment` reads them: every name that ends in
/// `_proxy` in any case, the last of them winning, then those that end in
/// lower case, which win over the others and unset them when empty.
fn named(variables: impl IntoIterator<Item = (String, String)>) -> HashMap<String, String> {
    let mut proxies = HashMap::new();
    let mut read = Vec::new();
    let mut cgi = false;
    for (name, value) in variables {
        cgi |= name == CGI;
        let Some((scheme, ending)) = name.rsplit_once('_') else {
            continue;
        };
        if !ending.eq_ignore_ascii_case("proxy") {
            continue;
        }
        let scheme = scheme.to_lowercase();
        if !value.is_empty() {
            proxies.insert(scheme.clone(), value.clone());
        }
        read.push((ending == "proxy", scheme, value));
    }
    if cgi {
        proxies.remove("http");
    }
    for (_, scheme, value) in read.into_iter().filter(|(lower, ..)| *lower) {
        if value.is_empty() {
            proxies.remove(&scheme);
        } else {
            proxies.insert(scheme, value);
        }
    }
    proxies
}

/// The pattern an entry of `NO_PROXY` stands for, as Python's
/// `get_environment_proxies` writes it: an entry written as a URL is the
/// pattern itself, an address is one host, as is `localhost`, and any
/// other name stands for itself and what is under it.
fn exempted(entry: &str) -> String {
    let before_a_slash = entry.split('/').next().unwrap_or(entry);
    if entry.contains("://") {
        entry.to_owned()
    } else if before_a_slash.parse::<std::net::Ipv4Addr>().is_ok() {
        format!("all://{entry}")
    } else if before_a_slash.parse::<std::net::Ipv6Addr>().is_ok() {
        match entry.split_once('/') {
            Some((address, range)) => format!("all://[{address}]/{range}"),
            None => format!("all://[{entry}]"),
        }
    } else if entry.eq_ignore_ascii_case("localhost") {
        format!("all://{entry}")
    } else {
        format!("all://*{entry}")
    }
}

/// `text` without the white space at its ends, as Python's `str.strip`
/// takes it: the four separators of the C0 controls are white space there.
fn stripped(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// The proxy `written` names, as the HTTP client takes it: its scheme, its
/// user information, its host and its port, without its path.
fn proxy_of(written: &str) -> Result<reqwest::Url, Failure> {
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
    let host = match read.host {
        Host::Name(name) | Host::International(name) => name,
        Host::V6(address, _) => format!("[{address}]"),
        Host::Absent => return Err(unread(Unread)),
    };
    let credentials = match read.user_information {
        "" => String::new(),
        written => format!("{written}@"),
    };
    let port = match read.port.map(u16::try_from) {
        Some(Ok(port)) => format!(":{port}"),
        Some(Err(_)) => return Err(unread(Unread)),
        None => String::new(),
    };
    let url = format!("{}://{credentials}{host}{port}", read.scheme);
    reqwest::Url::parse(&url).map_err(|_| unread(Unread))
}

impl Proxies {
    /// What the environment names now.
    ///
    /// # Errors
    ///
    /// The failure of a provider that cannot be built, for an environment
    /// Python's client refuses when it is built, or that this one does not
    /// read.
    pub(crate) fn of_the_environment() -> Result<Self, Failure> {
        let text = |value: std::ffi::OsString| value.to_string_lossy().into_owned();
        Self::read(std::env::vars_os().map(|(name, value)| (text(name), text(value))))
    }

    /// What `variables` name, the variables of an environment in its order.
    pub(crate) fn read(
        variables: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self, Failure> {
        let named = named(variables);
        let exempt = named.get("no").map(String::as_str).unwrap_or_default();
        let exempt: Vec<&str> = exempt.split(',').map(stripped).collect();
        // A star exempts everything, before any other text is read.
        if exempt.contains(&"*") {
            return Ok(Self::default());
        }
        // A rule written twice keeps its first place and its last proxy,
        // as the dictionary of Python's client does.
        let mut mounted: Vec<(String, Option<String>)> = Vec::new();
        let mut mount = |pattern: String, proxy: Option<String>| match mounted
            .iter_mut()
            .find(|(written, _)| *written == pattern)
        {
            Some((_, held)) => *held = proxy,
            None => mounted.push((pattern, proxy)),
        };
        for scheme in SCHEMES {
            if let Some(proxy) = named.get(scheme) {
                let has_a_scheme = proxy.contains("://");
                let proxy = if has_a_scheme {
                    proxy.clone()
                } else {
                    format!("http://{proxy}")
                };
                mount(format!("{scheme}://"), Some(proxy));
            }
        }
        for entry in exempt.into_iter().filter(|entry| !entry.is_empty()) {
            mount(exempted(entry), None);
        }
        let mut rules = Vec::with_capacity(mounted.len());
        for (pattern, proxy) in mounted {
            let unread =
                |_| Failure::unbuilt("an entry of NO_PROXY in the environment does not read");
            rules.push(Rule {
                pattern: Pattern::of(&pattern).map_err(unread)?,
                proxy: proxy.as_deref().map(proxy_of).transpose()?,
            });
        }
        // The sort keeps the order of the rules of one rank.
        rules.sort_by_key(|rule| rule.pattern.rank);
        Ok(Self { rules })
    }

    /// The proxy a request for `url` is sent through: `None` when it is
    /// sent directly.
    pub(crate) fn for_url(&self, url: &reqwest::Url) -> Option<reqwest::Url> {
        let rule = self.rules.iter().find(|rule| rule.pattern.applies_to(url));
        rule.and_then(|rule| rule.proxy.clone())
    }

    /// The URLs of the proxies, each of which may carry credentials.
    pub(crate) fn urls(&self) -> impl Iterator<Item = &reqwest::Url> {
        self.rules.iter().filter_map(|rule| rule.proxy.as_ref())
    }
}

#[cfg(test)]
mod tests;
