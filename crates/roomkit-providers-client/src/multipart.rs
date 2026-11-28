//! A `multipart/form-data` body (RFC 7578), as an endpoint that takes files
//! is sent one: the images API's edits take their references and their mask
//! so. A provider builds its parts and sends the body this module writes,
//! with the `Content-Type` it names; a replay reads a body back into its
//! parts.

use std::collections::hash_map::RandomState;
use std::fmt::Write as _;
use std::hash::BuildHasher as _;
use std::sync::atomic::{AtomicU64, Ordering};

/// One part of a body: a field, or a file with its name and its media type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// The name the part is sent under.
    pub name: String,
    /// The file's name, for a part that is a file.
    pub filename: Option<String>,
    /// The file's media type, for a part that is a file.
    pub content_type: Option<String>,
    /// The part's value: a field's text, a file's bytes.
    pub data: Vec<u8>,
}

impl Part {
    /// The field `name` holding `text`.
    pub fn field(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            filename: None,
            content_type: None,
            data: text.into().into_bytes(),
        }
    }

    /// The file `filename` of the media type `content_type`, holding
    /// `data`, under `name`.
    pub fn file(
        name: impl Into<String>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        data: Vec<u8>,
    ) -> Self {
        Self {
            name: name.into(),
            filename: Some(filename.into()),
            content_type: Some(content_type.into()),
            data,
        }
    }
}

/// Why a body cannot be written: a part's media type holds a character
/// that would end its header and start another, which Python's client
/// refuses in a header's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unwritable;

impl std::fmt::Display for Unwritable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a part of the body holds a control character in its header")
    }
}

impl std::error::Error for Unwritable {}

/// The `Content-Type` of a multipart body, and the body of `parts`, in
/// order, under a boundary drawn at random, as Python's client draws one.
///
/// # Errors
///
/// [`Unwritable`] for a part whose name, file name or media type holds a
/// control character.
pub fn encoded(parts: &[Part]) -> Result<(String, Vec<u8>), Unwritable> {
    let boundary = boundary();
    let mut body = Vec::new();
    for part in parts {
        if part
            .content_type
            .as_deref()
            .is_some_and(|value| value.chars().any(forbidden))
        {
            return Err(Unwritable);
        }
        let mut head = format!("--{boundary}\r\n");
        let _ = write!(
            head,
            "Content-Disposition: form-data; name=\"{}\"",
            quoted(&part.name)
        );
        if let Some(filename) = &part.filename {
            let _ = write!(head, "; filename=\"{}\"", quoted(filename));
        }
        head.push_str("\r\n");
        if let Some(content_type) = &part.content_type {
            let _ = write!(head, "Content-Type: {content_type}\r\n");
        }
        head.push_str("\r\n");
        body.extend_from_slice(head.as_bytes());
        body.extend_from_slice(&part.data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok((format!("multipart/form-data; boundary={boundary}"), body))
}

/// A name or a file name within its quotes, as Python's client writes it
/// (HTML's form encoding): a quote as `%22`, a backslash doubled, and a
/// control character but the escape as `%XX`.
fn quoted(name: &str) -> String {
    let mut quoted = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '"' => quoted.push_str("%22"),
            '\\' => quoted.push_str("\\\\"),
            '\u{0}'..='\u{1a}' | '\u{1c}'..='\u{1f}' => {
                let _ = write!(quoted, "%{:02X}", u32::from(c));
            }
            other => quoted.push(other),
        }
    }
    quoted
}

/// Whether `c` is refused in a header's value, as Python's client refuses
/// it: a control character but a tab.
fn forbidden(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{a}'..='\u{1f}' | '\u{7f}')
}

/// A boundary of 128 bits drawn at random, as Python's client draws one
/// (`os.urandom(16)`): the hashes of a counter under keys the process
/// draws from the system, which no part's bytes can foresee.
fn boundary() -> String {
    static DRAWN: AtomicU64 = AtomicU64::new(0);
    let drawn = DRAWN.fetch_add(1, Ordering::Relaxed);
    let keys = RandomState::new();
    let [high, low] = [0_u64, 1].map(|half| keys.hash_one((drawn, half)));
    format!("{high:016x}{low:016x}")
}

/// The parts of `body`, a body whose `Content-Type` is `content_type`, in
/// order; `None` for a body that is not one: what a replay reads of a body
/// sent.
#[cfg(any(test, feature = "harness"))]
#[must_use]
pub fn parsed(body: &[u8], content_type: &str) -> Option<Vec<Part>> {
    let boundary = content_type
        .split(';')
        .map(str::trim)
        .find_map(|parameter| parameter.strip_prefix("boundary="))?
        .trim_matches('"');
    let delimiter = format!("--{boundary}");
    let between = format!("\r\n{delimiter}");
    let mut parts = Vec::new();
    let mut rest = body.strip_prefix(delimiter.as_bytes())?;
    loop {
        if rest.starts_with(b"--") {
            return Some(parts);
        }
        rest = rest.strip_prefix(b"\r\n")?;
        let end = find(rest, between.as_bytes())?;
        parts.push(part_of(rest.get(..end)?)?);
        rest = rest.get(end + between.len()..)?;
    }
}

/// One part, its headers then its value.
#[cfg(any(test, feature = "harness"))]
fn part_of(raw: &[u8]) -> Option<Part> {
    let split = find(raw, b"\r\n\r\n")?;
    let headers = std::str::from_utf8(raw.get(..split)?).ok()?;
    let data = raw.get(split + 4..)?.to_vec();
    let (mut name, mut filename, mut content_type) = (None, None, None);
    for line in headers.split("\r\n") {
        let (header, value) = line.split_once(':')?;
        match header.trim().to_ascii_lowercase().as_str() {
            "content-disposition" => {
                name = parameter(value, "name");
                filename = parameter(value, "filename");
            }
            "content-type" => content_type = Some(value.trim().to_owned()),
            _ => {}
        }
    }
    Some(Part {
        name: name?,
        filename,
        content_type,
        data,
    })
}

/// The quoted parameter `wanted` of a `Content-Disposition`.
#[cfg(any(test, feature = "harness"))]
fn parameter(value: &str, wanted: &str) -> Option<String> {
    value.split(';').map(str::trim).find_map(|parameter| {
        let (key, quoted) = parameter.split_once('=')?;
        (key.trim() == wanted).then(|| quoted.trim().trim_matches('"').to_owned())
    })
}

/// Where `needle` first starts in `haystack`.
#[cfg(any(test, feature = "harness"))]
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_reads_back_as_its_parts() {
        let parts = vec![
            Part::field("prompt", "paint the sky"),
            Part::field("n", "1"),
            Part::file(
                "image[]",
                "reference-0.png",
                "image/png",
                b"\x89PNG\r\n--".to_vec(),
            ),
        ];
        let (content_type, body) = encoded(&parts).unwrap();
        assert!(content_type.starts_with("multipart/form-data; boundary="));
        assert_eq!(parsed(&body, &content_type), Some(parts));
    }

    #[test]
    fn each_body_draws_its_own_boundary() {
        let parts = [Part::field("n", "1")];
        let (first, _) = encoded(&parts).unwrap();
        let (second, _) = encoded(&parts).unwrap();
        assert_ne!(first, second);
    }

    // A media type that would end its header and start another is refused,
    // as Python's client refuses one; a tab is kept.
    #[test]
    fn a_control_character_in_a_media_type_is_refused() {
        let injected =
            "image/png\r\n\r\nx\r\n--boundary\r\nContent-Disposition: form-data; name=\"n\"";
        let part =
            |media_type: &str| Part::file("image[]", "reference-0.png", media_type, b"x".to_vec());
        assert_eq!(encoded(&[part(injected)]), Err(Unwritable));
        assert_eq!(encoded(&[part("image/png\u{7f}")]), Err(Unwritable));
        assert!(encoded(&[part("image/png;\tq=1")]).is_ok());
    }

    // A name and a file name are written as Python's client writes them: a
    // quote, a backslash and a control character encoded, never a line
    // break that would start a header.
    #[test]
    fn a_name_is_encoded_as_python_s_client_encodes_it() {
        assert_eq!(quoted("a\"b\\c\r\nd\u{1b}"), "a%22b\\\\c%0D%0Ad\u{1b}");
        let part = Part::file("image[]", "a\r\nb.png", "image/png", b"x".to_vec());
        let (content_type, body) = encoded(std::slice::from_ref(&part)).unwrap();
        let read = parsed(&body, &content_type).unwrap();
        assert_eq!(read[0].filename.as_deref(), Some("a%0D%0Ab.png"));
    }

    #[test]
    fn a_body_that_is_no_multipart_is_none() {
        assert_eq!(parsed(b"{}", "application/json"), None);
        assert_eq!(parsed(b"garbage", "multipart/form-data; boundary=x"), None);
    }
}
