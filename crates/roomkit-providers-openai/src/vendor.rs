//! The vendors whose providers speak `OpenAI`'s Chat Completions with rules
//! of their own (RFC §6.4, §6.7): where a vendor reads a model's earlier
//! reasoning, how it takes a reasoning setting, the tool names it accepts,
//! what its usage reports. Each is Python's provider class of that vendor;
//! [`crate::OpenAi`] is `OpenAI`'s.

use roomkit::model::Metadata;
use roomkit_ai::{AiContext, ModelInfo};
use roomkit_providers_client::endpoint::is_vendor_endpoint;
use serde_json::{Value, json};

use crate::dialect::{
    CEREBRAS_CHAT, ChatDialect, DEEPSEEK_CHAT, KIMI_CHAT, MINIMAX_CHAT, OPENAI_CHAT,
};
use crate::endpoint::OPENAI_BASE_URL;
use crate::failure::Failure;
use crate::models::{self, reasoning_floor};
use crate::names::{self, NameRule};
use crate::reasoning::{floored_effort, nearest_level, thinking_switch};

mod arguments;

pub(crate) use arguments::{repaired_response, repaired_stream};

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
/// The ids of Meta's chat models, among the image, speech and segmentation
/// models its listing holds too (Python's `_CHAT_PREFIXES`).
const META_CHAT_PREFIXES: [&str; 3] = ["muse-spark", "muse-wonder", "muse-ember"];
/// The least effort every Muse chat model takes: Meta's service cannot turn
/// reasoning off, and answers `none` with a 400.
const META_FLOOR: &str = "minimal";

/// The efforts on the scale a vendor's levels are read on, least to most.
const SCALE: [&str; 6] = ["minimal", "low", "medium", "high", "xhigh", "max"];
/// The efforts MiniMax-M3.1-Flash takes (MiniMax's API reference).
const MINIMAX_LEVELS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
/// The efforts K3 takes (Kimi's API reference).
const KIMI_LEVELS: [&str; 3] = ["low", "high", "max"];

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
            Self::OpenAi | Self::LlamaCpp { .. } | Self::Xai | Self::Qwen { .. } | Self::Meta => {
                OPENAI_CHAT
            }
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
            | Self::Meta => true,
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
            | Self::Meta => None,
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
            Self::LlamaCpp { .. } | Self::Xai | Self::Qwen { .. } | Self::Meta => true,
            Self::Cerebras { .. } => false,
            Self::OpenAi => {
                !self.is_own_endpoint(base_url) || models::sees_images_by_its_name(model)
            }
            Self::DeepSeek { .. } => models::sees_images_by_its_name(model),
        }
    }

    /// What a vendor's request carries of the turn's reasoning, on a turn
    /// with tools as on any other, `entry` the model's catalogue entry and
    /// `configured` the configuration's effort (Python's
    /// `_apply_sampling_kwargs` of each vendor's provider). `OpenAI`'s is
    /// the request's own ([`crate::request::Asked`]).
    pub(crate) fn sampling(
        self,
        entry: Option<&ModelInfo>,
        request: &AiContext,
        configured: Option<&str>,
    ) -> Sampling {
        match self {
            Self::OpenAi => Sampling::default(),
            Self::LlamaCpp { enable_thinking } => local_template(request, enable_thinking),
            Self::DeepSeek { enable_thinking } => deepseek(request, configured, enable_thinking),
            Self::MiniMax => minimax(entry, request, configured),
            Self::Kimi => Sampling {
                effort: levelled(entry, request, configured, &KIMI_LEVELS),
                extra: Metadata::new(),
            },
            Self::Xai => Sampling {
                effort: xai(entry, request, configured),
                extra: Metadata::new(),
            },
            Self::Cerebras {
                reasoning_format,
                clear_thinking,
            } => cerebras(entry, request, configured, reasoning_format, clear_thinking),
            Self::Qwen { enable_thinking } => qwen(request, enable_thinking),
            Self::Meta => Sampling {
                effort: floored_effort(request, configured, Some(META_FLOOR)).map(str::to_owned),
                extra: Metadata::new(),
            },
        }
    }
}

/// Local chat templates consume thinking settings on tool turns as well.
fn local_template(request: &AiContext, default: Option<bool>) -> Sampling {
    let mut template = serde_json::Map::new();
    if let Some(enabled) = thinking_switch(request).or(default) {
        template.insert("enable_thinking".into(), enabled.into());
    }
    if let Some(effort) = request.reasoning_effort.as_deref().filter(|s| *s != "none") {
        template.insert("reasoning_effort".into(), effort.into());
    }
    let mut sampling = Sampling::default();
    if !template.is_empty() {
        sampling
            .extra
            .insert("chat_template_kwargs".into(), Value::Object(template));
    }
    sampling
}

/// The effort on a vendor's catalogued model: the turn's over the
/// configured one, its floor where the turn states off, else the nearest
/// of the `levels` it takes; on a model the catalogue does not name, as
/// stated.
fn levelled(
    entry: Option<&ModelInfo>,
    request: &AiContext,
    configured: Option<&str>,
    levels: &[&str],
) -> Option<String> {
    let effort = floored_effort(request, configured, reasoning_floor(entry));
    if entry.is_none() {
        return effort.map(str::to_owned);
    }
    nearest_level(effort, levels, &SCALE).map(str::to_owned)
}

/// MiniMax's: M3, whose floor is `none`, stops reasoning through its own
/// `thinking` switch, its `reasoning_effort` being M3.1-Flash's alone.
fn minimax(entry: Option<&ModelInfo>, request: &AiContext, configured: Option<&str>) -> Sampling {
    let floor = reasoning_floor(entry);
    let effort = floored_effort(request, configured, floor);
    if effort == Some("none") && floor == Some("none") {
        let mut extra = Metadata::new();
        extra.insert("thinking".to_owned(), json!({ "type": "disabled" }));
        return Sampling {
            effort: None,
            extra,
        };
    }
    Sampling {
        effort: levelled(entry, request, configured, &MINIMAX_LEVELS),
        extra: Metadata::new(),
    }
}

/// xAI's: the turn's effort over the configured one, on a turn with tools
/// as on any other, withheld from a model the catalogue describes without
/// `thinking`, which would reject it (Python's
/// `XAIAIProvider._supports_reasoning`); a model it does not know, or knows
/// with no tags, reasons.
fn xai(entry: Option<&ModelInfo>, request: &AiContext, configured: Option<&str>) -> Option<String> {
    let reasons = entry.is_none_or(|entry| {
        entry.capabilities.is_empty() || entry.capabilities.iter().any(|tag| tag == "thinking")
    });
    let effort = request.reasoning_effort.as_deref().or(configured);
    effort.filter(|_| reasons).map(str::to_owned)
}

/// Cerebras's: the effort floored by the model's catalogue entry, on a turn
/// with tools too, and the configuration's `reasoning_format` and
/// `clear_thinking` where it states them.
fn cerebras(
    entry: Option<&ModelInfo>,
    request: &AiContext,
    configured: Option<&str>,
    reasoning_format: Option<&'static str>,
    clear_thinking: Option<bool>,
) -> Sampling {
    let effort = floored_effort(request, configured, reasoning_floor(entry)).map(str::to_owned);
    let mut extra = Metadata::new();
    if let Some(format) = reasoning_format {
        extra.insert("reasoning_format".to_owned(), Value::from(format));
    }
    if let Some(clear) = clear_thinking {
        extra.insert("clear_thinking".to_owned(), Value::from(clear));
    }
    Sampling { effort, extra }
}

/// Qwen's `enable_thinking` and `thinking_budget`, outside `OpenAI`'s
/// schema (Python's `QwenAIProvider._resolve_thinking`): the turn's switch
/// over the configured one, nothing when neither is stated; a positive
/// budget caps the trace of a turn that thinks. Qwen has no effort tier: an
/// effort counts only as `none`, which turns thinking off.
fn qwen(request: &AiContext, enable_thinking: Option<bool>) -> Sampling {
    let mut extra = Metadata::new();
    if let Some(enabled) = thinking_switch(request).or(enable_thinking) {
        extra.insert("enable_thinking".to_owned(), Value::from(enabled));
        if let Some(budget) = request
            .thinking_budget
            .filter(|budget| enabled && *budget > 0)
        {
            extra.insert("thinking_budget".to_owned(), Value::from(budget));
        }
    }
    Sampling {
        effort: None,
        extra,
    }
}

/// DeepSeek's `thinking` object, its effort nested in it, never a top-level
/// `reasoning_effort`, which DeepSeek ignores (Python's
/// `DeepSeekAIProvider._resolve_thinking`): the turn's switch over the
/// configured `enable_thinking`, off as `disabled`; nothing when neither the
/// switch nor an effort is stated, so that the model's default applies.
fn deepseek(
    request: &AiContext,
    configured: Option<&str>,
    enable_thinking: Option<bool>,
) -> Sampling {
    let enabled = thinking_switch(request).or(enable_thinking);
    let effort = request.reasoning_effort.as_deref().or(configured);
    let thinking = match (enabled, effort) {
        (Some(false), _) => Some(json!({ "type": "disabled" })),
        (None, None) => None,
        (_, effort) => {
            let mut object = serde_json::Map::new();
            object.insert("type".to_owned(), Value::from("enabled"));
            if let Some(effort) = effort {
                object.insert("reasoning_effort".to_owned(), Value::from(effort));
            }
            Some(Value::Object(object))
        }
    };
    let mut extra = Metadata::new();
    if let Some(thinking) = thinking {
        extra.insert("thinking".to_owned(), thinking);
    }
    Sampling {
        effort: None,
        extra,
    }
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
