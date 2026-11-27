//! Gemini's thinking config for one turn (Python's `thinking_config`): the
//! turn states whether the model thinks and how much, the configured level
//! what it leaves unstated (RFC §6.7). Gemini takes a level or a token
//! budget, never both; `include_thoughts` is what streams the thought
//! summaries. The SDK writes the config's members in snake case and a level
//! in capitals.

use roomkit_ai::AiContext;
use serde_json::{Value, json};

use crate::models::{CANNOT_STOP, TAKES_LEVELS, TAKES_MINIMAL};

/// The efforts on the scale a model's levels are read on, least to most.
const SCALE: [&str; 5] = ["minimal", "low", "medium", "high", "xhigh"];
/// The levels of a Gemini 3 model.
const LEVELS: [&str; 3] = ["low", "medium", "high"];
/// The levels of a Gemini 3 model that takes `minimal`.
const LEVELS_MINIMAL: [&str; 4] = ["minimal", "low", "medium", "high"];

/// The thinking config of `request` for a model whose catalogue entry
/// declares `capabilities`, with the configuration's `configured` level;
/// `None` for the model's default.
pub(crate) fn config(
    request: &AiContext,
    configured: Option<&str>,
    capabilities: &[String],
) -> Option<Value> {
    let budget = request.thinking_budget;
    // A negative budget is Gemini's dynamic one (-1): on, the model sizing it.
    let dynamic = budget.is_some_and(|budget| budget < 0);
    let configured_on = configured.is_some_and(|level| !level.is_empty());
    let switch = if dynamic {
        Some(true)
    } else {
        switch(request, configured_on.then_some(true))
    };
    let levels = levels(configured, capabilities);
    if switch == Some(false) {
        return Some(off(levels, capabilities));
    }
    let stated = configured.filter(|level| !level.is_empty());
    let level = nearest_level(request.reasoning_effort.as_deref(), levels).or(stated);
    if let Some(level) = level {
        return Some(
            json!({"include_thoughts": true, "thinking_level": level.to_ascii_uppercase()}),
        );
    }
    if let Some(budget) = budget.filter(|budget| *budget != 0) {
        return Some(json!({"include_thoughts": true, "thinking_budget": budget}));
    }
    // Thinking on, the model sizing it: what turns it on for a model that
    // thinks by default not at all.
    (switch == Some(true)).then(|| json!({"include_thoughts": true, "thinking_budget": -1}))
}

/// Whether the model reasons on this turn, as the turn states it, else as
/// `configured` does (Python's `thinking_switch`).
fn switch(request: &AiContext, configured: Option<bool>) -> Option<bool> {
    if let Some(budget) = request.thinking_budget {
        return Some(budget > 0);
    }
    if request.enable_thinking.is_some() {
        return request.enable_thinking;
    }
    if request.reasoning_effort.as_deref() == Some("none") {
        return Some(false);
    }
    configured
}

/// The levels the model takes, least to most: from the catalogue, or the
/// usual three where a level is configured for a model it does not carry.
fn levels(configured: Option<&str>, capabilities: &[String]) -> &'static [&'static str] {
    let tagged = |tag: &str| capabilities.iter().any(|capability| capability == tag);
    if tagged(TAKES_MINIMAL) {
        &LEVELS_MINIMAL
    } else if tagged(TAKES_LEVELS) || configured.is_some() {
        &LEVELS
    } else {
        &[]
    }
}

/// Reasoning off: a budget of 0, or the lowest level for a model that
/// cannot stop reasoning, which answers 400 to that budget.
fn off(levels: &[&str], capabilities: &[String]) -> Value {
    let cannot_stop = capabilities
        .iter()
        .any(|capability| capability == CANNOT_STOP);
    match levels.first() {
        Some(lowest) if cannot_stop => {
            json!({"include_thoughts": false, "thinking_level": lowest.to_ascii_uppercase()})
        }
        _ => json!({"include_thoughts": false, "thinking_budget": 0}),
    }
}

/// The level of `levels` nearest to `effort` on the scale, the lower one on
/// a tie; `None` for an effort outside the scale or a model without levels
/// (Python's `nearest_level`).
fn nearest_level<'a>(effort: Option<&str>, levels: &[&'a str]) -> Option<&'a str> {
    let rank = |level: &str| SCALE.iter().position(|step| *step == level);
    let wanted = rank(effort?)?;
    levels
        .iter()
        .copied()
        .filter_map(|level| rank(level).map(|at| (wanted.abs_diff(at), at, level)))
        .min_by_key(|(distance, at, _)| (*distance, *at))
        .map(|(_, _, level)| level)
}
