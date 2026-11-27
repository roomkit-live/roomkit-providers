//! The vendors whose providers speak `OpenAI`'s Chat Completions with rules
//! of their own (RFC §6.4, §6.7): where a vendor reads a model's earlier
//! reasoning, how it takes a reasoning setting, the tool names it accepts,
//! what its usage reports. Each is Python's provider class of that vendor;
//! [`crate::OpenAi`] is `OpenAI`'s.

use roomkit::model::Metadata;
use roomkit_ai::ModelInfo;
use roomkit_providers_client::endpoint::is_vendor_endpoint;

use crate::dialect::{
    CEREBRAS_CHAT, ChatDialect, DEEPSEEK_CHAT, KIMI_CHAT, MINIMAX_CHAT, OPENAI_CHAT,
};
use crate::endpoint::OPENAI_BASE_URL;
use crate::failure::Failure;
use crate::models;
use crate::names::{self, NameRule};

mod arguments;
mod listings;
mod sampling;

pub(crate) use arguments::{repaired_response, repaired_stream};
pub(crate) use listings::MODEL_INFO;
pub(crate) use sampling::litellm_sampling;

/// DeepSeek's endpoint, under the two bases its documentation gives.
pub(crate) const DEEPSEEK_BASE_URLS: [&str; 2] =
    ["https://api.deepseek.com/v1", "https://api.deepseek.com"];
/// MiniMax's OpenAI-compatible endpoint.
pub(crate) const MINIMAX_BASE_URL: &str = "https://api.minimax.io/v1";
/// Kimi's OpenAI-compatible endpoint.
pub(crate) const KIMI_BASE_URL: &str = "https://api.moonshot.ai/v1";
/// xAI's endpoint.
pub(crate) const XAI_BASE_URL: &str = "https://api.x.ai/v1";
/// Cerebras's endpoint.
pub(crate) const CEREBRAS_BASE_URL: &str = "https://api.cerebras.ai/v1";
/// Alibaba Model Studio's OpenAI-compatible endpoint, international.
pub(crate) const QWEN_BASE_URL: &str = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1";
/// Meta's endpoint.
pub(crate) const META_BASE_URL: &str = "https://api.meta.ai/v1";
/// OpenRouter's endpoint.
pub(crate) const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
/// A LiteLLM proxy on its own default port, which the proxy serves with or
/// without `/v1`.
pub(crate) const LITELLM_BASE_URL: &str = "http://localhost:4000";
/// The ids of Meta's chat models, among the image, speech and segmentation
/// models its listing holds too (Python's `_CHAT_PREFIXES`).
const META_CHAT_PREFIXES: [&str; 3] = ["muse-spark", "muse-wonder", "muse-ember"];
/// The vendor a provider applies the rules of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Vendor {
    /// `OpenAI`, or any server behind a base URL: Python's `OpenAIAIProvider`.
    OpenAi,
    /// The local llama.cpp server and its chat-template thinking switch.
    LlamaCpp { enable_thinking: Option<bool> },
    /// Python's `DeepSeekAIProvider`, with its configuration's
    /// `enable_thinking`.
    DeepSeek { enable_thinking: Option<bool> },
    /// Python's `MiniMaxAIProvider`.
    MiniMax,
    /// Python's `KimiAIProvider`.
    Kimi,
    /// Python's `XAIAIProvider`.
    Xai,
    /// Python's `CerebrasAIProvider`, with its configuration's
    /// `reasoning_format` and `clear_thinking`.
    Cerebras {
        reasoning_format: Option<&'static str>,
        clear_thinking: Option<bool>,
    },
    /// Python's `QwenAIProvider`, with its configuration's `enable_thinking`.
    Qwen { enable_thinking: Option<bool> },
    /// Python's `MetaAIProvider`.
    Meta,
    /// Python's `OpenRouterAIProvider`.
    OpenRouter,
    /// Python's `LiteLLMAIProvider`, in front of a LiteLLM proxy.
    LiteLlm,
}

/// What a request carries of the turn's reasoning beside `OpenAI`'s fields:
/// its `reasoning_effort`, and the vendor's own members, which win over a
/// configuration's `extra_body` as Python merges the turn's last.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Sampling {
    pub(crate) effort: Option<String>,
    pub(crate) extra: Metadata,
}

impl Vendor {
    /// Where the vendor reads a replayed round's reasoning.
    pub(crate) fn dialect(self) -> ChatDialect<'static> {
        match self {
            Self::OpenAi
            | Self::LlamaCpp { .. }
            | Self::Xai
            | Self::Qwen { .. }
            | Self::Meta
            | Self::OpenRouter
            | Self::LiteLlm => OPENAI_CHAT,
            Self::DeepSeek { .. } => DEEPSEEK_CHAT,
            Self::MiniMax => MINIMAX_CHAT,
            Self::Kimi => KIMI_CHAT,
            Self::Cerebras { .. } => CEREBRAS_CHAT,
        }
    }

    /// The provider's name, as Python's class names itself.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAIAIProvider",
            Self::LlamaCpp { .. } => "llamacpp",
            Self::DeepSeek { .. } => "DeepSeekAIProvider",
            Self::MiniMax => "minimax",
            Self::Kimi => "kimi",
            Self::Xai => "XAIAIProvider",
            Self::Cerebras { .. } => "cerebras",
            Self::Qwen { .. } => "QwenAIProvider",
            Self::Meta => "MetaAIProvider",
            Self::OpenRouter => "OpenRouterAIProvider",
            Self::LiteLlm => "LiteLLMAIProvider",
        }
    }

    /// The provider an error names (Python's `_provider_name`).
    pub(crate) fn provider(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::LlamaCpp { .. } => "llamacpp",
            Self::DeepSeek { .. } => "deepseek",
            Self::MiniMax => "minimax",
            Self::Kimi => "kimi",
            Self::Xai => "xai",
            Self::Cerebras { .. } => "cerebras",
            Self::Qwen { .. } => "qwen",
            Self::Meta => "meta",
            Self::OpenRouter => "openrouter",
            Self::LiteLlm => "litellm",
        }
    }

    /// The vendor's curated catalogue, Python's (`catalog/`).
    pub(crate) fn models(self) -> &'static [ModelInfo] {
        models::of(self)
    }

    /// Whether the vendor's endpoint honours a `json_schema` response format
    /// when the configuration does not say (Python's
    /// `_response_schema_default`): DeepSeek's, MiniMax's and Qwen's do not
    /// document one for their models, the others' do.
    pub(crate) fn takes_response_schema(self) -> bool {
        match self {
            Self::OpenAi
            | Self::Kimi
            | Self::LlamaCpp { .. }
            | Self::Xai
            | Self::Cerebras { .. }
            | Self::Meta
            | Self::OpenRouter
            | Self::LiteLlm => true,
            Self::DeepSeek { .. } | Self::MiniMax | Self::Qwen { .. } => false,
        }
    }

    /// The vendor's own endpoint, however its documentation writes it.
    pub(crate) fn base_urls(self) -> &'static [&'static str] {
        match self {
            Self::OpenAi => &[OPENAI_BASE_URL],
            Self::LlamaCpp { .. } => &[],
            Self::DeepSeek { .. } => &DEEPSEEK_BASE_URLS,
            Self::MiniMax => &[MINIMAX_BASE_URL],
            Self::Kimi => &[KIMI_BASE_URL],
            Self::Xai => &[XAI_BASE_URL],
            Self::Cerebras { .. } => &[CEREBRAS_BASE_URL],
            Self::Qwen { .. } => &[QWEN_BASE_URL],
            Self::Meta => &[META_BASE_URL],
            Self::OpenRouter => &[OPENROUTER_BASE_URL],
            Self::LiteLlm => &[LITELLM_BASE_URL],
        }
    }

    /// Whether `base_url` is the vendor's own endpoint, whose rules the
    /// provider knows (Python's `is_vendor_endpoint`).
    pub(crate) fn is_own_endpoint(self, base_url: Option<&str>) -> bool {
        is_vendor_endpoint(base_url, self.base_urls())
    }

    /// The tool names the vendor's own endpoint accepts; none where the
    /// vendor states no rule. Behind another base URL the server decides
    /// (RFC §6.7), which the caller leaves unchecked.
    pub(crate) fn name_rule(self) -> Option<&'static NameRule> {
        match self {
            Self::OpenAi => Some(&names::OPENAI),
            Self::DeepSeek { .. } => Some(&names::DEEPSEEK),
            Self::Kimi => Some(&names::KIMI),
            Self::MiniMax
            | Self::LlamaCpp { .. }
            | Self::Xai
            | Self::Cerebras { .. }
            | Self::Qwen { .. }
            | Self::Meta
            | Self::OpenRouter
            | Self::LiteLlm => None,
        }
    }

    /// Whether the vendor reports its cache hits as DeepSeek does, apart
    /// from `OpenAI`'s `prompt_tokens_details`.
    pub(crate) fn reads_cache_hits(self) -> bool {
        matches!(self, Self::DeepSeek { .. })
    }

    /// Whether the vendor may report reasoning beside its completion, billed
    /// as output: xAI's total is the prompt, the completion and the
    /// reasoning, where `OpenAI`'s reasoning sits inside the completion.
    pub(crate) fn adds_reasoning_reported_apart(self) -> bool {
        matches!(self, Self::Xai)
    }

    /// Whether the vendor has no listing to ask, its catalogue being what an
    /// account reaches (Python's `QwenAIProvider.list_models`).
    pub(crate) fn lists_its_catalogue(self) -> bool {
        matches!(self, Self::Qwen { .. })
    }

    /// Whether a model the listing gives is one the vendor's provider
    /// keeps: Meta's listing holds its image, speech and segmentation models
    /// too, of which the provider keeps the chat ones.
    pub(crate) fn keeps_listed(self, model: &str) -> bool {
        if !matches!(self, Self::Meta) {
            return true;
        }
        META_CHAT_PREFIXES
            .iter()
            .any(|prefix| model.starts_with(prefix))
    }

    /// Whether a request that constrains its answer asks the vendor to
    /// route it only where every parameter it sends is honoured:
    /// OpenRouter may otherwise send it to an upstream that drops
    /// `response_format` without a word (Python's
    /// `OpenRouterAIProvider._apply_response_format`).
    pub(crate) fn requires_parameters(self) -> bool {
        matches!(self, Self::OpenRouter)
    }

    /// The path of the vendor's model listing under its base URL.
    pub(crate) fn listing_path(self) -> &'static str {
        if self == Self::LiteLlm {
            return MODEL_INFO;
        }
        "/models"
    }

    /// Whether the vendor writes an array argument as the JSON text of the
    /// array, which the provider reads back where the tool declares one.
    pub(crate) fn repairs_arguments(self) -> bool {
        matches!(self, Self::Cerebras { .. })
    }

    /// Whether the configured `model` reads images: as the vendor's
    /// catalogue states it of its `entry`; else, on `OpenAI`, MiniMax and
    /// Kimi, the server's to decide behind a base URL (RFC §6.7), and on
    /// `OpenAI`'s own endpoint and on DeepSeek, which keeps `OpenAI`'s
    /// reading, as `OpenAI`'s model names say (Python's `supports_vision`
    /// of each provider, and `_VISION_PREFIXES`).
    pub(crate) fn sees_images(
        self,
        entry: Option<&ModelInfo>,
        model: &str,
        base_url: Option<&str>,
    ) -> bool {
        if let Some(vision) = entry.and_then(|entry| entry.supports_vision) {
            return vision;
        }
        match self {
            Self::MiniMax | Self::Kimi => !self.is_own_endpoint(base_url),
            Self::LlamaCpp { .. }
            | Self::Xai
            | Self::Qwen { .. }
            | Self::Meta
            | Self::OpenRouter
            | Self::LiteLlm => true,
            Self::Cerebras { .. } => false,
            Self::OpenAi => {
                !self.is_own_endpoint(base_url) || models::sees_images_by_its_name(model)
            }
            Self::DeepSeek { .. } => models::sees_images_by_its_name(model),
        }
    }
}

/// `body` asking OpenRouter to route it only to an upstream that honours
/// every parameter it sends, beside the routing preferences it already
/// states, which are kept.
pub(crate) fn require_parameters(body: &mut serde_json::Map<String, serde_json::Value>) {
    let stated = body.get("provider").and_then(serde_json::Value::as_object);
    let mut provider = stated.cloned().unwrap_or_default();
    provider.insert("require_parameters".to_owned(), true.into());
    body.insert("provider".to_owned(), provider.into());
}

mod macros;

pub(crate) use macros::{into_openai, vendor_config_form, vendor_provider};

#[cfg(test)]
mod tests;

/// The failure a vendor's configuration naming a reasoning field is refused
/// with, as Python's validator refuses it (RFC §6.4).
pub(crate) fn refused_thinking_field(config: &str) -> Failure {
    Failure::before_request(format!(
        "{config} takes no thinking_field: its provider sends reasoning where its vendor reads it"
    ))
}

/// Why a key is refused where the configuration takes ASCII only: Python's
/// `_header_safe`, the key itself never said.
pub(crate) const NOT_ASCII: &str = "api_key holds a non-ASCII character (a placeholder such as '\u{2026}' \
     copied as is?): an HTTP Authorization header cannot carry it";

/// The failure a configuration that takes ASCII only is refused with when
/// its key is not.
pub(crate) fn refused_key() -> Failure {
    Failure::before_request(NOT_ASCII.to_owned())
}

/// The failure a vendor's configuration is refused with: Python's validator
/// on a field the vendor's provider would ignore, since it applies its
/// vendor's own rule on a turn with tools (RFC §6.7).
pub(crate) fn refused_tool_turn_setting(config: &str) -> Failure {
    Failure::before_request(format!(
        "{config} takes no supports_reasoning_effort_with_tools: its provider applies its \
         vendor's own rule on a turn with tools"
    ))
}
