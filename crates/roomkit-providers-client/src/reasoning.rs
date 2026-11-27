//! The reasoning a turn asks of its model against what a provider's
//! configuration states, as every provider reads it (RFC §6.7): Python's
//! `providers/ai/reasoning.py`.

use roomkit_ai::AiContext;

/// The efforts a turn may ask for, least to most; `none` is off, not a
/// level (Python's `_EFFORTS`): the scale levels are read on, unless a
/// vendor's runs further.
pub const EFFORTS: [&str; 5] = ["minimal", "low", "medium", "high", "xhigh"];

/// Whether the model reasons on a turn, as the turn states it: its
/// `thinking_budget` first (0 or a negative one off, above 0 on), then
/// `enable_thinking`, and a `reasoning_effort` of `none` states off. What
/// the turn leaves unstated falls to `configured`, the switch the
/// provider's configuration carries; `None` when nothing states it
/// (Python's `thinking_switch`).
#[must_use]
pub fn thinking_switch(request: &AiContext, configured: Option<bool>) -> Option<bool> {
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

/// The effort a turn sends a model whose least effort is `floor`: the
/// turn's over the `configured` one, and `floor` where the turn states off
/// (its budget or its switch, or an effort of `none`), the model's off
/// value or, on one that cannot stop reasoning, its lowest level. Without a
/// floor the effort goes as stated (Python's `floored_effort`).
#[must_use]
pub fn floored_effort<'a>(
    request: &'a AiContext,
    configured: Option<&'a str>,
    floor: Option<&'a str>,
) -> Option<&'a str> {
    let effort = request.reasoning_effort.as_deref().or(configured);
    let off = thinking_switch(request, None) == Some(false) || effort == Some("none");
    floor.filter(|_| off).or(effort)
}

/// The level of `levels` (least to most) nearest to `effort` on `scale`, the
/// lower one on a tie; `None` for an effort outside `scale`, `none`
/// included, or a model that takes no level (Python's `nearest_level`, RFC
/// §6.7).
#[must_use]
pub fn nearest_level<'a>(
    effort: Option<&str>,
    levels: &[&'a str],
    scale: &[&str],
) -> Option<&'a str> {
    let rank = |level: &str| scale.iter().position(|step| *step == level);
    let wanted = rank(effort?)?;
    levels
        .iter()
        .copied()
        .filter_map(|level| rank(level).map(|at| (wanted.abs_diff(at), at, level)))
        .min_by_key(|(distance, at, _)| (*distance, *at))
        .map(|(_, _, level)| level)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Python: tests/test_providers/test_reasoning_settings.py::test_the_switch_reads_the_budget_then_enable_thinking_then_an_effort_of_none
    #[test]
    fn the_switch_reads_the_budget_then_enable_thinking_then_an_effort_of_none() {
        let switch = |thinking_budget, enable_thinking, effort: Option<&str>| {
            let request = AiContext {
                thinking_budget,
                enable_thinking,
                reasoning_effort: effort.map(str::to_owned),
                ..AiContext::default()
            };
            thinking_switch(&request, None)
        };
        assert_eq!(switch(Some(0), Some(true), None), Some(false));
        assert_eq!(switch(Some(512), None, Some("none")), Some(true));
        assert_eq!(switch(None, Some(true), Some("none")), Some(true));
        assert_eq!(switch(None, None, Some("none")), Some(false));
        assert_eq!(switch(Some(-1), None, None), Some(false));
        assert_eq!(switch(None, None, Some("low")), None);
    }

    // Python: providers/ai/reasoning.py thinking_switch: what the turn leaves
    // unstated falls to the configured switch, and only that.
    #[test]
    fn what_the_turn_leaves_unstated_falls_to_the_configured_switch() {
        let unstated = AiContext::default();
        assert_eq!(thinking_switch(&unstated, Some(true)), Some(true));
        assert_eq!(thinking_switch(&unstated, Some(false)), Some(false));
        let off = AiContext {
            enable_thinking: Some(false),
            ..AiContext::default()
        };
        assert_eq!(thinking_switch(&off, Some(true)), Some(false));
    }
}
