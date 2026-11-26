//! Configuration shared with RoomKit's Python llama.cpp provider.

use std::{fmt, path::PathBuf, time::Duration};

use roomkit_ai::ProviderError;
use serde::{Deserialize, Serialize};

use crate::{binary, error};

/// Settings of one managed server. Only the model is required.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, remote = "Self")]
pub struct LlamaCppConfig {
    /// A local GGUF path or a Hugging Face repository with an optional quantization.
    pub model: String,
    /// Context capacity in tokens, including tool declarations and results.
    pub context_size: u32,
    /// Layers to offload to the GPU. Zero selects the CPU.
    pub gpu_layers: Option<u32>,
    /// Default reply token limit.
    pub max_tokens: u32,
    /// The template's default thinking switch.
    pub enable_thinking: Option<bool>,
    /// Explicit server executable, by path or name on PATH.
    pub binary: Option<PathBuf>,
    /// Pinned release variant to install instead of detecting the hardware.
    pub variant: Option<String>,
    /// Directory for pinned builds. Defaults to the RoomKit cache.
    pub cache_dir: Option<PathBuf>,
    /// Local listening port, or an automatically allocated port.
    pub port: Option<u16>,
    /// Time to allow the child to download/load the model and answer its health check.
    #[serde(with = "seconds")]
    pub startup_timeout: Duration,
    /// HTTP read timeout for a model request.
    #[serde(with = "seconds")]
    pub timeout: Duration,
    /// Additional arguments passed directly to the server executable.
    pub extra_args: Vec<String>,
}

impl Default for LlamaCppConfig {
    fn default() -> Self {
        Self::new("")
    }
}

impl LlamaCppConfig {
    /// A model with the default settings of RoomKit's Python provider.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            context_size: 8192,
            gpu_layers: None,
            max_tokens: 1024,
            enable_thinking: None,
            binary: None,
            variant: None,
            cache_dir: None,
            port: None,
            startup_timeout: Duration::from_mins(30),
            timeout: Duration::from_mins(2),
            extra_args: Vec::new(),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), ProviderError> {
        if self.model.trim().is_empty() {
            return Err(error(
                "model must name a GGUF file or a Hugging Face repository",
            ));
        }
        if self.context_size == 0 || self.max_tokens == 0 || self.port == Some(0) {
            return Err(error("context_size, max_tokens and port must be positive"));
        }
        if self.timeout.is_zero()
            || self.startup_timeout.is_zero()
            || std::time::Instant::now()
                .checked_add(self.startup_timeout)
                .is_none()
        {
            return Err(error("timeouts must be positive and fit the clock"));
        }
        if let Some(variant) = &self.variant {
            binary::release()?.assets(variant)?;
        }
        Ok(())
    }
}

impl fmt::Debug for LlamaCppConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlamaCppConfig")
            .field("context_size", &self.context_size)
            .field("variant", &self.variant)
            .field("port", &self.port)
            .finish_non_exhaustive()
    }
}

mod seconds {
    use super::Duration;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub(super) fn serialize<S: Serializer>(value: &Duration, out: S) -> Result<S::Ok, S::Error> {
        out.serialize_f64(value.as_secs_f64())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<Duration, D::Error> {
        Duration::try_from_secs_f64(f64::deserialize(input)?).map_err(D::Error::custom)
    }
}

impl Serialize for LlamaCppConfig {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        LlamaCppConfig::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for LlamaCppConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let config = LlamaCppConfig::deserialize(deserializer)?;
        config.validate().map_err(serde::de::Error::custom)?;
        Ok(config)
    }
}
