//! The Developer API's endpoint and paths (Python's `DEVELOPER_API_VERSION`,
//! as the google-genai SDK writes its URLs).

/// The Developer API, at its version.
pub(crate) const DEVELOPER_API: &str = "https://generativelanguage.googleapis.com/v1beta";

/// The header a request carries its key in.
pub(crate) const KEY_HEADER: &str = "x-goog-api-key";
