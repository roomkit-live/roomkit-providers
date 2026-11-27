//! An Azure endpoint as it is pasted, reduced to the resource's base
//! (Python's `normalize_azure_endpoint`): Azure hands out the resource's
//! root, the v1 API's URL and a deployment's whole target URI, and the
//! provider appends its own path to the first only. The query, the
//! fragment, the trailing slashes and a suffix the provider writes again go;
//! the scheme and the host stay as they came, an API Management gateway's
//! `/openai` prefix among them.

/// The suffixes a pasted URL loses, which no resource's base ends with:
/// each written in any case, a slash after it or not.
const SUFFIXES: [&str; 3] = ["/openai/v1", "/chat/completions", "/responses"];

/// The suffix of a deployment's path, its name after it.
const DEPLOYMENTS: &str = "/openai/deployments";

/// `value`, the endpoint a configuration states, as the resource's base;
/// `None` for one Python's URL reader refuses, an IPv6 host left unclosed
/// among them. A text that is no URL with a host is left as it is, its
/// trailing slashes aside.
pub(crate) fn resource_base(value: &str) -> Option<String> {
    let raw = value.trim_matches(is_python_space);
    if raw.is_empty() {
        return Some(String::new());
    }
    let (scheme, netloc, path) = match split(raw) {
        Split::Refused => return None,
        Split::NoHost => return Some(raw.trim_end_matches('/').to_owned()),
        Split::Url {
            scheme,
            netloc,
            path,
        } => (scheme, netloc, path),
    };
    let mut path = path.as_str();
    // A whole target URI carries two suffixes, the v1 API's form one under
    // another: up to four go.
    for _ in 0..4 {
        let stripped = without_suffix(path);
        if stripped.len() == path.len() {
            break;
        }
        path = stripped;
    }
    Some(format!("{scheme}://{netloc}{}", path.trim_end_matches('/')))
}

/// What Python's `str.strip()` strips: the white space Rust knows, and the
/// four separators of information, U+001C to U+001F.
fn is_python_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// What Python's `urlsplit` reads of a URL.
enum Split {
    /// A URL `urlsplit` refuses.
    Refused,
    /// A text with no scheme or no authority.
    NoHost,
    /// The scheme in lower case, the authority and the path, without the
    /// query and the fragment.
    Url {
        scheme: String,
        netloc: String,
        path: String,
    },
}

/// `url` as Python's `urlsplit` splits it.
///
/// What `urlsplit` checks of an authority past ASCII once normalized
/// (`_checknetloc`) is not read: such an authority is taken as written.
fn split(url: &str) -> Split {
    let url: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let (scheme, rest) = match url.split_once(':') {
        Some((scheme, rest)) if is_scheme(scheme) => (Some(scheme), rest),
        _ => (None, url.as_str()),
    };
    // An authority follows `//`, a scheme before it or not, and its
    // brackets are checked either way.
    let Some(rest) = rest.strip_prefix("//") else {
        return Split::NoHost;
    };
    let (netloc, after) = rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len()));
    if check_brackets(netloc).is_none() {
        return Split::Refused;
    }
    let Some(scheme) = scheme.filter(|_| !netloc.is_empty()) else {
        return Split::NoHost;
    };
    let path = after.split('#').next().unwrap_or_default();
    let path = path.split('?').next().unwrap_or_default();
    Split::Url {
        scheme: scheme.to_ascii_lowercase(),
        netloc: netloc.to_owned(),
        path: path.to_owned(),
    }
}

/// Whether `text` is a URL's scheme: an ASCII letter, then letters, digits,
/// `+`, `-` and `.`.
fn is_scheme(text: &str) -> bool {
    let signs = |c: char| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.');
    text.starts_with(|c: char| c.is_ascii_alphabetic()) && text.chars().all(signs)
}

/// `Some` for an authority whose brackets `urlsplit` takes: none, or a
/// pair holding an IPv6 address or an `IPvFuture` one, nothing before the
/// opening one but user information, and only a port after the closing one.
fn check_brackets(netloc: &str) -> Option<()> {
    let (opens, closes) = (netloc.contains('['), netloc.contains(']'));
    if opens != closes {
        return None;
    }
    if !opens {
        return Some(());
    }
    let host_and_port = netloc.rsplit_once('@').map_or(netloc, |(_, host)| host);
    let (before, bracketed) = host_and_port.split_once('[')?;
    if !before.is_empty() {
        return None;
    }
    let (host, port) = bracketed.split_once(']').unwrap_or((bracketed, ""));
    if !port.is_empty() && !port.starts_with(':') {
        return None;
    }
    if let Some(future) = host.strip_prefix('v') {
        let (version, address) = future.split_once('.')?;
        let hexadecimal = !version.is_empty() && version.chars().all(|c| c.is_ascii_hexdigit());
        return (hexadecimal && !address.is_empty()).then_some(());
    }
    let address = host.split_once('%').map_or(host, |(address, _)| address);
    address.parse::<std::net::Ipv6Addr>().ok().map(|_| ())
}

/// `path` without the suffix of its end a pasted URL carries and the
/// provider writes again (Python's `_REWRITABLE_SUFFIX`), a slash after it
/// or not, in any case: the leftmost such suffix that reaches the end, as a
/// regular expression finds it.
fn without_suffix(path: &str) -> &str {
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    let ends_with = |text: &str, suffix: &str| {
        let end = text
            .len()
            .checked_sub(suffix.len())
            .and_then(|at| text.get(at..));
        end.is_some_and(|end| end.eq_ignore_ascii_case(suffix))
    };
    let fixed = SUFFIXES
        .iter()
        .filter(|suffix| ends_with(trimmed, suffix))
        .map(|suffix| trimmed.len() - suffix.len());
    let deployment = trimmed.rsplit_once('/').and_then(|(before, name)| {
        let named = !name.is_empty() && ends_with(before, DEPLOYMENTS);
        named.then(|| before.len() - DEPLOYMENTS.len())
    });
    let start = fixed.chain(deployment).min();
    start.and_then(|start| path.get(..start)).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leftmost_suffix_goes_as_a_regular_expression_finds_it() {
        // `/openai/deployments/<name>` starts before `/responses` here.
        assert_eq!(without_suffix("/a/openai/deployments/responses"), "/a");
        assert_eq!(without_suffix("/a/OpenAI/Deployments/x/"), "/a");
        assert_eq!(
            without_suffix("/a/openai/deployments/"),
            "/a/openai/deployments/"
        );
        assert_eq!(without_suffix("/openai"), "/openai");
    }

    #[test]
    fn brackets_are_read_as_urlsplit_reads_them() {
        assert!(check_brackets("[::1]:8080").is_some());
        assert!(check_brackets("user@[fe80::1%eth0]").is_some());
        assert!(check_brackets("[v1.fe]").is_some());
        assert!(check_brackets("[::1").is_none());
        assert!(check_brackets("host[::1]").is_none());
        assert!(check_brackets("[127.0.0.1]").is_none());
        assert!(check_brackets("[::1]x").is_none());
    }
}
