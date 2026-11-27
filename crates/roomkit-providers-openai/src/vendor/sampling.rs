//! What a vendor's request carries of the turn's reasoning (Python's
//! `_apply_sampling_kwargs` of each vendor's provider): an effort, or the
//! vendor's own members, on a turn with tools as on any other.

use roomkit::model::Metadata;
use roomkit_ai::{AiContext, ModelInfo};
use serde_json::{Value, json};

use super::{Sampling, Vendor};
use crate::models::reasoning_floor;
use crate::reasoning::{floored_effort, nearest_level, thinking_switch};

/// The least effort every Muse chat model takes: Meta's service cannot turn
/// reasoning off, and answers `none` with a 400.
const META_FLOOR: &str = "minimal";

/// The efforts on the scale a vendor's levels are read on, least to most.
pub(super) const SCALE: [&str; 6] = ["minimal", "low", "medium", "high", "xhigh", "max"];
/// The efforts MiniMax-M3.1-Flash takes (MiniMax's API reference).
const MINIMAX_LEVELS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
/// The efforts K3 takes (Kimi's API reference).
pub(super) const KIMI_LEVELS: [&str; 3] = ["low", "high", "max"];

impl Vendor {
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
            // `OpenAI`'s, LiteLLM's, Azure's and vLLM's read the configuration,
            // which the request applies.
            Self::OpenAi | Self::LiteLlm | Self::Azure | Self::Vllm => Sampling::default(),
            Self::OpenRouter => openrouter(request, configured),
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
    if let Some(budget) = request.thinking_budget {
        // RoomKit treats nonpositive budgets as off. llama.cpp uses -1 for unlimited.
        sampling
            .extra
            .insert("reasoning_budget_tokens".into(), budget.max(0).into());
    }
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

/// OpenRouter's `reasoning` object, which it translates for every upstream
/// it routes to, in place of a top-level `reasoning_effort` (Python's
/// `OpenRouterAIProvider._resolve_reasoning`): off as `{"enabled": false}`;
/// on, a positive budget as its `max_tokens`, else the effort, the turn's
/// over the configured one, else `{"enabled": true}` for a turn that states
/// on; nothing when the turn states nothing, so that the model decides.
fn openrouter(request: &AiContext, configured: Option<&str>) -> Sampling {
    let switch = thinking_switch(request);
    let budget = request.thinking_budget.filter(|budget| *budget != 0);
    let effort = request.reasoning_effort.as_deref().or(configured);
    let reasoning = match (switch, budget, effort.filter(|effort| !effort.is_empty())) {
        (Some(false), _, _) => Some(json!({ "enabled": false })),
        (_, Some(budget), _) => Some(json!({ "max_tokens": budget })),
        (_, None, Some(effort)) => Some(json!({ "effort": effort })),
        (Some(true), None, None) => Some(json!({ "enabled": true })),
        (None, None, None) => None,
    };
    let mut extra = Metadata::new();
    if let Some(reasoning) = reasoning {
        extra.insert("reasoning".to_owned(), reasoning);
    }
    Sampling {
        effort: None,
        extra,
    }
}

/// LiteLLM's, which it translates for every upstream it fronts (Python's
/// `LiteLLMAIProvider._apply_sampling_kwargs`): nothing for a turn that
/// states off, as LiteLLM has no off that every translator takes; a positive
/// budget as an Anthropic `thinking` object, else the effort, the turn's
/// over the configured one, as `reasoning_effort`. On a turn with tools
/// either goes only where `says`, the configuration's
/// `supports_reasoning_effort_with_tools`, is true, the model behind the
/// alias being unknown; an unsaid one is `warned` of.
pub(crate) fn litellm_sampling(
    request: &AiContext,
    configured: Option<&str>,
    says: Option<bool>,
    warned: impl FnOnce(&str),
) -> Sampling {
    let mut sampling = Sampling::default();
    if thinking_switch(request) == Some(false) {
        return sampling;
    }
    let budget = request.thinking_budget.filter(|budget| *budget != 0);
    let effort = request.reasoning_effort.as_deref().or(configured);
    if !request.tools.is_empty() {
        let setting = budget.map(|budget| budget.to_string());
        let setting = setting.or_else(|| effort.map(str::to_owned));
        if let (None, Some(setting)) = (says, &setting) {
            warned(setting);
        }
        if says != Some(true) {
            return sampling;
        }
    }
    if let Some(budget) = budget {
        let thinking = json!({ "type": "enabled", "budget_tokens": budget });
        sampling.extra.insert("thinking".to_owned(), thinking);
        return sampling;
    }
    sampling.effort = effort.map(str::to_owned);
    sampling
}
