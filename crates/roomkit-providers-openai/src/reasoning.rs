//! The reasoning a turn asks of its model, as every vendor of the Chat
//! Completions wire reads it (RFC §6.7): Python's
//! `providers/ai/reasoning.py`.

use roomkit_ai::AiContext;

/// Whether the model reasons on a turn, as the turn states it: its
/// `thinking_budget` first (0 or a negative one off, above 0 on), then
/// `enable_thinking`, and a `reasoning_effort` of `none` states off; `None`
/// when nothing states it (Python's `thinking_switch`).
pub(crate) fn thinking_switch(request: &AiContext) -> Option<bool> {
    if let Some(budget) = request.thinking_budget {
        return Some(budget > 0);
    }
    if request.enable_thinking.is_some() {
        return request.enable_thinking;
    }
    (request.reasoning_effort.as_deref() == Some("none")).then_some(false)
}

/// The effort a turn sends a model whose least effort is `floor`: the
/// turn's over the `configured` one, and `floor` where the turn states off
/// (its budget or its switch, or an effort of `none`), the model's off
/// value or, on one that cannot stop reasoning, its lowest level. Without a
/// floor the effort goes as stated (Python's `floored_effort`).
pub(crate) fn floored_effort<'a>(
    request: &'a AiContext,
    configured: Option<&'a str>,
    floor: Option<&'a str>,
) -> Option<&'a str> {
    let effort = request.reasoning_effort.as_deref().or(configured);
    let off = thinking_switch(request) == Some(false) || effort == Some("none");
    floor.filter(|_| off).or(effort)
}

/// The level of `levels` (least to most) nearest to `effort` on `scale`, the
/// lower one on a tie; `None` for an effort outside `scale`, `none`
/// included, or a model that takes no level (Python's `nearest_level`, RFC
/// §6.7).
pub(crate) fn nearest_level<'a>(
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
            thinking_switch(&AiContext {
                thinking_budget,
                enable_thinking,
                reasoning_effort: effort.map(str::to_owned),
                ..AiContext::default()
            })
        };
        assert_eq!(switch(Some(0), Some(true), None), Some(false));
        assert_eq!(switch(Some(512), None, Some("none")), Some(true));
        assert_eq!(switch(None, Some(true), Some("none")), Some(true));
        assert_eq!(switch(None, None, Some("none")), Some(false));
        assert_eq!(switch(Some(-1), None, None), Some(false));
        assert_eq!(switch(None, None, Some("low")), None);
    }
}
