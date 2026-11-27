//! Mistral's endpoint and paths, as the mistralai SDK writes its URLs
//! (Python's `MISTRAL_BASE_URL`).

/// Mistral's own endpoint, which a configuration without a server URL asks.
pub(crate) const MISTRAL_BASE_URL: &str = "https://api.mistral.ai";

/// Where a chat completion is asked, under the base URL.
pub(crate) const CHAT_PATH: &str = "/v1/chat/completions";

/// Where the models are listed, under the base URL.
pub(crate) const MODELS_PATH: &str = "/v1/models";
