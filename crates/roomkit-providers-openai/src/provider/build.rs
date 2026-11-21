//! What a provider is built from: where it sends, what every request
//! carries, the times it gives a request, and what no failure may repeat
//! of its configuration.

use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};

use super::JSON;
use crate::config::OpenAiConfig;
use crate::endpoint::is_openai_endpoint;
use crate::failure::Failure;
use crate::failure::exit::Secrets;

/// Compiles only while the URL the client dials is the `url` crate's, the
/// one `roomkit-providers-http` validates a destination with: a URL that
/// was checked there is read the same way here.
const fn the_client_s_url(url: reqwest::Url) -> url::Url {
    url
}

/// The failure of a provider that cannot be built: before any request,
/// final.
pub(super) fn refused(reason: &'static str) -> Failure {
    Failure::before_request(format!("the provider cannot be built: {reason}"))
}

/// The timeouts a client is built with: the configuration's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Timeouts {
    /// The longest wait for a read: for the head of an answer from the
    /// moment its request is sent, then between two reads of its body.
    pub(crate) read: Duration,
    /// The longest wait for a connection.
    pub(crate) connect: Duration,
}

/// How long past its timeout a clock must still count: a provider outlives
/// no such time, so a timeout the clock can add once it can add always.
const HEADROOM: Duration = Duration::from_hours(100 * 365 * 24);

impl Timeouts {
    /// The configuration's timeouts.
    ///
    /// # Errors
    ///
    /// A timeout so long that the clock cannot count to its end: the HTTP
    /// client would fail on it at the first read.
    pub(super) fn of(config: &OpenAiConfig) -> Result<Self, Failure> {
        let counted = |timeout: Duration| {
            let end = Instant::now().checked_add(timeout);
            end.and_then(|end| end.checked_add(HEADROOM)).is_some()
        };
        if !counted(config.timeout) || !counted(config.connect_timeout) {
            return Err(refused("its timeout is longer than the clock can count"));
        }
        Ok(Self {
            read: config.timeout,
            connect: config.connect_timeout,
        })
    }
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
pub(super) fn joined(base_url: &str, path: &str) -> Result<reqwest::Url, Failure> {
    let not_http = || refused("its base URL is not an http or https URL");
    if base_url
        .chars()
        .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(refused(
            "its base URL holds white space or a control character",
        ));
    }
    if base_url.contains('\\') {
        return Err(refused("its base URL holds a backslash"));
    }
    let lowered = base_url.to_ascii_lowercase();
    let after_the_scheme = lowered.strip_prefix("https://");
    let after_the_scheme = after_the_scheme.or_else(|| lowered.strip_prefix("http://"));
    if after_the_scheme.ok_or_else(not_http)?.starts_with('/') {
        return Err(refused("its base URL names no host"));
    }
    let base = reqwest::Url::parse(base_url).map_err(|_| not_http())?;
    if base.query().is_some() || base.fragment().is_some() {
        return Err(refused(
            "its base URL has a query or a fragment, which the path of the API cannot follow",
        ));
    }
    if let Some(reason) = read_otherwise(&lowered, &base) {
        return Err(refused(reason));
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
/// credentials.
fn has_user_information(url: &reqwest::Url) -> bool {
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
fn percent_decoded(text: &str) -> String {
    String::from_utf8_lossy(&percent_decoded_bytes(text)).into_owned()
}

/// What no failure may repeat of `config`: the key, the value of each
/// default header and its credentials when it names a scheme before them,
/// the user information of the base URL, as written and as it reads, and
/// the `Authorization` the `client` writes from that user information.
pub(super) fn secrets_of(
    config: &OpenAiConfig,
    client: &reqwest::Client,
    chat: &reqwest::Url,
) -> Secrets {
    let mut texts = vec![config.api_key.clone()];
    let with_credentials = |value: &str| {
        let after_a_scheme = value
            .split_once(' ')
            .map(|(_, rest)| rest.trim().to_owned());
        [Some(value.to_owned()), after_a_scheme]
    };
    let headers = config.default_headers.iter().flatten();
    texts.extend(
        headers
            .flat_map(|(_, value)| with_credentials(value))
            .flatten(),
    );
    if has_user_information(chat) {
        let (user, password) = (chat.username(), chat.password().unwrap_or_default());
        let read = [percent_decoded(user), percent_decoded(password)];
        texts.extend([user.to_owned(), password.to_owned()]);
        texts.push(format!("{user}:{password}"));
        texts.push(read.join(":"));
        texts.extend(read);
        // What the client itself writes of it on each request.
        let written = client.get(chat.clone()).build().ok();
        let written = written
            .as_ref()
            .and_then(|request| request.headers().get(AUTHORIZATION));
        let written = written.and_then(|value| value.to_str().ok());
        texts.extend(written.into_iter().flat_map(with_credentials).flatten());
    }
    Secrets::of(texts)
}

/// Whether the provider talks to `OpenAI`'s own endpoint, whose models and
/// rules it knows: the base URL is `OpenAI`'s as Python compares it, and
/// the URL the client dials is `OpenAI`'s as the client reads it.
pub(super) fn own_endpoint(config: &OpenAiConfig, dialed: &reqwest::Url) -> bool {
    let official = the_client_s_url(dialed.clone());
    is_openai_endpoint(config.base_url.as_deref())
        && official.scheme() == "https"
        && official.host_str() == Some("api.openai.com")
        && official.port_or_known_default() == Some(443)
        && official.path() == "/v1/chat/completions"
        && official.username().is_empty()
        && official.password().is_none()
}

/// The headers every request carries: what the provider writes, then the
/// configuration's, one of which replaces the provider's of the same name
/// as Python's client lets it. An empty key sends no `Authorization`. A
/// request with a body says beside them what the body is.
///
/// A base URL that holds user information is sent with it as Basic
/// credentials, which the client writes: as with Python's client, they
/// take the place of the key and of a configured `Authorization`, so
/// neither is written here.
pub(super) fn headers_of(config: &OpenAiConfig, chat: &reqwest::Url) -> Result<HeaderMap, Failure> {
    let basic = has_user_information(chat);
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static(JSON));
    if !config.api_key.is_empty() && !basic {
        let bearer = HeaderValue::from_str(&format!("Bearer {}", config.api_key));
        let mut bearer = bearer.map_err(|_| refused("its key cannot go in a header"))?;
        bearer.set_sensitive(true);
        headers.insert(AUTHORIZATION, bearer);
    }
    for (name, value) in config.default_headers.iter().flatten() {
        let no_header = || refused("one of its default headers is no HTTP header");
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| no_header())?;
        let mut value = HeaderValue::from_str(value).map_err(|_| no_header())?;
        if basic && name == AUTHORIZATION {
            continue;
        }
        value.set_sensitive(name == AUTHORIZATION);
        headers.insert(name, value);
    }
    Ok(headers)
}
