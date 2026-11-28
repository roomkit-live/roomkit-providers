//! A `multipart/form-data` body (RFC 7578), as an endpoint that takes files
//! is sent one: the images API's edits take their references and their mask
//! so. A provider builds its parts and sends the body this module writes,
//! with the `Content-Type` it names; a replay reads a body back into its
//! parts.

use std::fmt::Write as _;

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

/// The `Content-Type` of a multipart body, and the body of `parts`, in
/// order. Its boundary is one no part holds.
#[must_use]
pub fn encoded(parts: &[Part]) -> (String, Vec<u8>) {
    let boundary = boundary_of(parts);
    let mut body = Vec::new();
    for part in parts {
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
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// A name within its quotes: a quote and a line break escaped as HTML
/// forms escape them (RFC 7578 §4.2).
fn quoted(name: &str) -> String {
    name.replace('"', "%22")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// The first boundary of a series none of `parts` holds: of as many
/// boundaries as the parts have bytes and one more, one is held by none,
/// each place of a part's bytes starting one boundary at most.
fn boundary_of(parts: &[Part]) -> String {
    let holds = |boundary: &str| {
        let needle = boundary.as_bytes();
        parts.iter().any(|part| {
            let filename = part.filename.as_deref().unwrap_or("").as_bytes();
            [part.name.as_bytes(), filename, part.data.as_slice()]
                .into_iter()
                .any(|haystack| find(haystack, needle).is_some())
        })
    };
    let bytes: usize = parts
        .iter()
        .map(|part| {
            let filename = part.filename.as_ref().map_or(0, String::len);
            part.name.len() + filename + part.data.len()
        })
        .sum();
    (0..=bytes)
        .map(|at| format!("roomkit-providers-{at:08x}"))
        .find(|boundary| !holds(boundary))
        .unwrap_or_default()
}

/// The parts of `body`, a body whose `Content-Type` is `content_type`, in
/// order; `None` for a body that is not one.
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
fn parameter(value: &str, wanted: &str) -> Option<String> {
    value.split(';').map(str::trim).find_map(|parameter| {
        let (key, quoted) = parameter.split_once('=')?;
        (key.trim() == wanted).then(|| quoted.trim().trim_matches('"').to_owned())
    })
}

/// Where `needle` first starts in `haystack`.
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
        let (content_type, body) = encoded(&parts);
        assert!(content_type.starts_with("multipart/form-data; boundary="));
        assert_eq!(parsed(&body, &content_type), Some(parts));
    }

    #[test]
    fn the_boundary_is_one_no_part_holds() {
        let held = "roomkit-providers-00000000";
        let parts = vec![Part::field("text", held)];
        let (content_type, body) = encoded(&parts);
        assert!(!content_type.ends_with(held), "{content_type}");
        assert_eq!(parsed(&body, &content_type), Some(parts));
    }

    #[test]
    fn a_body_that_is_no_multipart_is_none() {
        assert_eq!(parsed(b"{}", "application/json"), None);
        assert_eq!(parsed(b"garbage", "multipart/form-data; boundary=x"), None);
    }
}
