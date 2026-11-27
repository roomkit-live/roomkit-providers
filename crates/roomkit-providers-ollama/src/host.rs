//! The URL a configured host stands for, as the ollama SDK reads it
//! (`_parse_host`): a scheme, `http` when none is written; a host, its user
//! information left out, `127.0.0.1` when none is written; a port, the
//! scheme's (80, 443) when one is written, else Ollama's 11434; a path, its
//! slashes trimmed. An IPv6 address is written as the URL parser writes it,
//! compressed, where Python keeps it as it was given: the same address.

/// The port of a host written with no scheme.
const OLLAMA_PORT: u16 = 11434;

/// The host a URL that names none stands for.
const LOOPBACK: &str = "127.0.0.1";

/// The base URL `host` stands for, or `None` for one no URL reads from.
pub(crate) fn base_url(host: &str) -> Option<String> {
    let (scheme, hostport, port) = match host.split_once("://") {
        Some((scheme, hostport)) if !hostport.is_empty() => {
            let port = match scheme {
                "http" => 80,
                "https" => 443,
                _ => OLLAMA_PORT,
            };
            (scheme, hostport, port)
        }
        _ => ("http", host, OLLAMA_PORT),
    };
    // A host and a port with no name, or nothing at all, name the loopback.
    let named = hostport.starts_with([':', '/']) || hostport.is_empty();
    let hostport = if named {
        format!("{LOOPBACK}{hostport}")
    } else {
        hostport.to_owned()
    };
    let split = reqwest::Url::parse(&format!("{scheme}://{hostport}")).ok()?;
    let name = split.host_str().filter(|name| !name.is_empty());
    let name = name.unwrap_or(LOOPBACK);
    let port = split.port().unwrap_or(port);
    let path = split.path().trim_matches('/');
    if path.is_empty() {
        return Some(format!("{scheme}://{name}:{port}"));
    }
    Some(format!("{scheme}://{name}:{port}/{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Python: ollama._client._parse_host's examples, every one, the IPv6
    // address written compressed.
    #[test]
    fn a_host_reads_as_the_sdk_reads_it() {
        let read = |host: &str| base_url(host).unwrap();
        let v6 = "[1:2:3:4::1]";
        let written = "[0001:002:003:0004::1]";
        for (host, url) in [
            ("", "http://127.0.0.1:11434".to_owned()),
            ("1.2.3.4", "http://1.2.3.4:11434".to_owned()),
            (":56789", "http://127.0.0.1:56789".to_owned()),
            ("1.2.3.4:56789", "http://1.2.3.4:56789".to_owned()),
            ("http://1.2.3.4", "http://1.2.3.4:80".to_owned()),
            ("https://1.2.3.4", "https://1.2.3.4:443".to_owned()),
            ("https://1.2.3.4:56789", "https://1.2.3.4:56789".to_owned()),
            ("example.com", "http://example.com:11434".to_owned()),
            ("example.com:56789", "http://example.com:56789".to_owned()),
            ("http://example.com", "http://example.com:80".to_owned()),
            ("https://example.com", "https://example.com:443".to_owned()),
            (
                "https://example.com:56789",
                "https://example.com:56789".to_owned(),
            ),
            ("example.com/", "http://example.com:11434".to_owned()),
            ("example.com:56789/", "http://example.com:56789".to_owned()),
            (
                "example.com/path",
                "http://example.com:11434/path".to_owned(),
            ),
            (
                "example.com:56789/path",
                "http://example.com:56789/path".to_owned(),
            ),
            (
                "https://example.com:56789/path",
                "https://example.com:56789/path".to_owned(),
            ),
            (
                "example.com:56789/path/",
                "http://example.com:56789/path".to_owned(),
            ),
            (written, format!("http://{v6}:11434")),
            (&format!("{written}:56789"), format!("http://{v6}:56789")),
            (&format!("http://{written}"), format!("http://{v6}:80")),
            (&format!("https://{written}"), format!("https://{v6}:443")),
            (
                &format!("https://{written}:56789"),
                format!("https://{v6}:56789"),
            ),
            (&format!("{written}/"), format!("http://{v6}:11434")),
            (&format!("{written}:56789/"), format!("http://{v6}:56789")),
            (
                &format!("{written}/path"),
                format!("http://{v6}:11434/path"),
            ),
            (
                &format!("{written}:56789/path"),
                format!("http://{v6}:56789/path"),
            ),
            (
                &format!("https://{written}:56789/path"),
                format!("https://{v6}:56789/path"),
            ),
            (
                &format!("{written}:56789/path/"),
                format!("http://{v6}:56789/path"),
            ),
            (
                "https://user:secret@Example.COM",
                "https://example.com:443".to_owned(),
            ),
        ] {
            assert_eq!(read(host), url, "{host}");
        }
    }
}
