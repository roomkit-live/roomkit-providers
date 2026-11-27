//! An image as Ollama takes it: a base64 text (Python's
//! `_ollama_image_payload`, then the ollama SDK's `Image`).
//!
//! A data URI is read as every provider reads one, its payload checked and
//! written as canonical base64. Any other URL goes as the SDK reads it: a
//! path that names a file is read from the disk and sent as base64 (decided
//! as Python does it, e2903981); a path of a picture that names none is
//! refused; any other text Python's lenient base64 reader takes is sent as
//! it is, and the rest refused. The flows of `RoomKit` hand no path: an
//! event's media refuses one, and a tool's image is a data URI.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use roomkit_ai::{AiImagePart, image_part_base64};
use roomkit_providers_client::failure::{own, told};

use crate::failure::Failure;

/// The kinds of picture whose path, named and missing, the SDK refuses.
const PICTURES: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

/// The text `part` goes as.
///
/// # Errors
///
/// A data URI whose payload does not read, a missing picture, and a text
/// that is no base64: each before anything is sent.
pub(crate) async fn payload(part: &AiImagePart, provider: &str) -> Result<String, Failure> {
    if part.url.starts_with("data:") {
        let (_, data) = image_part_base64(part, provider)?;
        return Ok(data);
    }
    read_as_the_sdk_does(&part.url).await
}

/// `text` as the SDK's `Image` writes it.
async fn read_as_the_sdk_does(text: &str) -> Result<String, Failure> {
    // Whether a file is there is asked as the SDK asks it (`Path.exists`),
    // one metadata lookup: only a file is read, off the runtime's workers,
    // so that a text that names none (base64, a link) takes no thread.
    let named_a_file = std::fs::metadata(text).is_ok_and(|found| found.is_file());
    if named_a_file && let Ok(bytes) = tokio::fs::read(text).await {
        return Ok(STANDARD.encode(bytes));
    }
    let kind = text.rsplit('.').next().unwrap_or(text).to_lowercase();
    if PICTURES.contains(&kind.as_str()) {
        let said = vec![
            own("the image's file "),
            told(text, 200),
            own(" does not exist"),
        ];
        return Err(Failure::said(said, false, None));
    }
    if lenient_base64(text) {
        return Ok(text.to_owned());
    }
    let said = "Invalid image data, expected base64 string or path to image file";
    Err(Failure::said(vec![own(said)], false, None))
}

/// Whether Python's `base64.b64decode` takes `text`, as it reads by
/// default: ASCII only; a character no base64 holds skipped; a padding that
/// ends a group ending the data; and no group left with a character or two
/// past the last whole one (`binascii.a2b_base64`, not strict).
pub(crate) fn lenient_base64(text: &str) -> bool {
    if !text.is_ascii() {
        return false;
    }
    let (mut place, mut pads) = (0_u8, 0_u8);
    for byte in text.bytes() {
        if byte == b'=' {
            if place >= 2 {
                pads = pads.saturating_add(1);
                if place.saturating_add(pads) >= 4 {
                    return true;
                }
            }
            continue;
        }
        if !(byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/') {
            continue;
        }
        pads = 0;
        place = (place + 1) % 4;
    }
    place == 0
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct Fixture {
        images: Vec<Image>,
    }

    #[derive(Deserialize)]
    struct Image {
        given: String,
        sent: Option<String>,
        refused: Option<String>,
    }

    // Every text of `tests/fixtures/python_images.json`, written by
    // `scripts/generate_ollama_images.py`: sent as the SDK sends it, or
    // refused where it refuses it.
    #[tokio::test]
    async fn each_image_text_goes_as_the_sdk_sends_it() {
        let fixture: Fixture =
            serde_json::from_str(include_str!("../tests/fixtures/python_images.json")).unwrap();
        assert!(fixture.images.len() >= 36);
        for image in fixture.images {
            let read = read_as_the_sdk_does(&image.given).await;
            match (read, image.sent, image.refused) {
                (Ok(sent), Some(expected), None) => assert_eq!(sent, expected, "{}", image.given),
                (Err(_), None, Some(_)) => {}
                (read, ..) => panic!("{:?}: {}", image.given, read.is_ok()),
            }
        }
    }

    #[tokio::test]
    async fn a_path_that_names_a_file_is_read_and_sent_as_base64() {
        let path = std::env::temp_dir().join(format!("ollama-image-{}.png", std::process::id()));
        std::fs::write(&path, b"a local file").unwrap();
        let read = read_as_the_sdk_does(path.to_str().unwrap()).await;
        std::fs::remove_file(&path).unwrap();
        assert_eq!(read.unwrap(), STANDARD.encode(b"a local file"));
        let missing = read_as_the_sdk_does(path.to_str().unwrap()).await;
        assert!(missing.is_err());
    }
}
