//! The readings of an image part every provider needs (RFC §6.7): its
//! media type and bytes, its media type and base64, or the URI to forward.

use std::error::Error;

use super::parts::AiImagePart;
use crate::data_uri::{DataUriError, base64_of, parse_data_uri, to_data_uri};
use crate::error::{ProviderError, ProviderErrorOptions};

/// A remote URL where a `data:` URI was expected, by its scheme.
#[derive(Debug, thiserror::Error)]
#[error("expected a data: URI, got a {0} URL")]
struct RemoteUri(String);

/// The failure of an image part that does not read: not retryable, raised
/// from `cause`.
fn invalid_image(provider: &str, cause: impl Error + Send + Sync + 'static) -> ProviderError {
    ProviderError::new(
        format!("invalid image part: {cause}"),
        ProviderErrorOptions {
            provider: provider.into(),
            ..Default::default()
        },
    )
    .with_source(cause)
}

/// The media type and decoded bytes of an inline image: Python's
/// `image_part_payload(part, *, provider)`. A missing media type falls back
/// to the part's type, then to `image/png`.
///
/// # Errors
///
/// Returns a non-retryable [`ProviderError`] naming `provider` for a remote
/// URL or an invalid `data:` URI.
pub fn image_part_payload(
    part: &AiImagePart,
    provider: &str,
) -> Result<(String, Vec<u8>), ProviderError> {
    parse_data_uri(&part.url, part.mime_type.as_deref()).map_err(|cause| match cause {
        DataUriError::NotADataUri => {
            let scheme = part.url.split(':').next().unwrap_or_default();
            invalid_image(provider, RemoteUri(scheme.into()))
        }
        DataUriError::NoPayload | DataUriError::NotBase64 => invalid_image(provider, cause),
    })
}

/// The media type and canonical base64 payload of an inline image: Python's
/// `image_part_base64(part, *, provider)`, for an API that takes the two
/// apart.
///
/// # Errors
///
/// Returns the same [`ProviderError`] as [`image_part_payload`].
pub fn image_part_base64(
    part: &AiImagePart,
    provider: &str,
) -> Result<(String, String), ProviderError> {
    let (kind, data) = image_part_payload(part, provider)?;
    Ok((kind, base64_of(&data)))
}

/// The URI to forward for an image part: Python's `image_part_uri(part, *,
/// provider)`. A remote URL passes through untouched. An inline image is
/// decoded and rebuilt with its media type and canonical base64, so a
/// payload an encoder wrapped reaches the vendor on one line.
///
/// # Errors
///
/// Returns the same [`ProviderError`] as [`image_part_payload`] for an
/// invalid `data:` URI.
pub fn image_part_uri(part: &AiImagePart, provider: &str) -> Result<String, ProviderError> {
    if !part.url.starts_with("data:") {
        return Ok(part.url.clone());
    }
    let (kind, data) = image_part_payload(part, provider)?;
    Ok(to_data_uri(&data, &kind))
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    #[test]
    fn all_three_readers_agree_on_data_and_remote_uris() {
        let image = AiImagePart {
            url: "data:;base64,QUJD\nMTI".into(),
            mime_type: Some("image/png".into()),
        };
        assert_eq!(
            image_part_payload(&image, "demo").unwrap(),
            ("image/png".into(), b"ABC12".to_vec())
        );
        assert_eq!(
            image_part_base64(&image, "demo").unwrap(),
            ("image/png".into(), "QUJDMTI=".into())
        );
        assert_eq!(
            image_part_uri(&image, "demo").unwrap(),
            "data:image/png;base64,QUJDMTI="
        );
        let remote = AiImagePart {
            url: "https://example.test/image.png".into(),
            mime_type: None,
        };
        assert_eq!(image_part_uri(&remote, "demo").unwrap(), remote.url);
        assert_eq!(
            image_part_payload(&remote, "demo").unwrap_err().message,
            "invalid image part: expected a data: URI, got a https URL"
        );
        assert!(image_part_base64(&remote, "demo").is_err());
    }

    #[test]
    fn all_readers_report_the_provider_and_the_decoding_cause() {
        let part = AiImagePart {
            url: "data:image/png;base64,not*base64".into(),
            mime_type: None,
        };
        for error in [
            image_part_uri(&part, "sample").unwrap_err(),
            image_part_base64(&part, "sample").unwrap_err(),
            image_part_payload(&part, "sample").unwrap_err(),
        ] {
            assert_eq!(error.provider, "sample");
            assert!(!error.retryable);
            assert!(!error.before_request);
            assert_eq!(error.context_overflow, None);
            assert_eq!(
                error.message,
                format!("invalid image part: {}", error.source().unwrap())
            );
        }
    }
}
