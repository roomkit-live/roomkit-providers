//! The turn's reasoning settings as an Anthropic request spells them (RFC
//! §6.7): Python's `reasoning.py`. Whether the model reasons and how much:
//! the `thinking` block, the `output_config.effort` and whether a
//! temperature is sent, for the model the catalogue declares. A model it
//! does not carry is sent the turn's effort on Anthropic's levels, and
//! nothing where the turn states off.

use roomkit_providers_llm::{AiRequest, ModelInfo};
use serde_json::{Value, json};

use crate::config::AnthropicConfig;
use crate::models::{
    EFFORT_NO_XHIGH, EFFORT_REFUSED, EFFORT_UP_TO_HIGH, THINKING_OFF_BETWEEN_TOOLS,
    THINKING_OFF_DISABLED, THINKING_OFF_EFFORT_HIGH, reasoning_floor, tagged,
};

/// Anthropic's efforts, least to most, under the turn's `minimal`, which no
/// Claude model takes: it goes as `low`.
const SCALE: [&str; 6] = ["minimal", "low", "medium", "high", "xhigh", "max"];

/// The `thinking` block that turns reasoning off, by the catalogue tag
/// declaring it.
const OFF_BLOCKS: [(&str, &str); 2] = [
    (THINKING_OFF_DISABLED, "disabled"),
    (THINKING_OFF_BETWEEN_TOOLS, "between_tools"),
];

/// Whether the model reasons on this turn, as the turn states it (Python's
/// `thinking_switch`): its budget first, 0 or below off and above 0 on,
/// then its switch, then an effort of `none`, which states off. `None` when
/// nothing states it.
fn switch(request: &AiRequest) -> Option<bool> {
    match (request.thinking_budget, request.enable_thinking) {
        (Some(budget), _) => Some(budget > 0),
        (None, Some(enabled)) => Some(enabled),
        (None, None) => (request.reasoning_effort.as_deref() == Some("none")).then_some(false),
    }
}

/// What a turn sends of its reasoning beside its effort.
pub(crate) struct Sampling {
    /// The `thinking` block, when the turn sends one.
    pub(crate) thinking: Option<Value>,
    /// Whether the turn sends its temperature.
    pub(crate) temperature: bool,
}

/// The turn's `thinking` block and whether its temperature is sent (Python's
/// `thinking_or_temperature`). A turn that thinks sends no temperature,
/// which Anthropic ignores then: adaptively where the configuration says
/// so, else on its budget, and a turn without a budget does not think
/// there. Where the turn states off, a model that reasons when `thinking`
/// is left out is sent the block its `entry` declares.
pub(crate) fn sampling(
    config: &AnthropicConfig,
    request: &AiRequest,
    entry: Option<&ModelInfo>,
) -> Sampling {
    let switch = switch(request);
    if switch == Some(true) {
        let thinking = if config.thinks_adaptively() {
            Some(json!({"type": "adaptive", "display": "summarized"}))
        } else {
            let budget = request.thinking_budget.filter(|budget| *budget != 0);
            budget.map(|budget| json!({"type": "enabled", "budget_tokens": budget}))
        };
        if thinking.is_some() {
            return Sampling {
                thinking,
                temperature: false,
            };
        }
    }
    let off = OFF_BLOCKS
        .iter()
        .find(|(tag, _)| switch == Some(false) && tagged(entry, tag));
    Sampling {
        thinking: off.map(|(_, kind)| json!({"type": kind})),
        temperature: config.takes_temperature(),
    }
}

/// The `output_config.effort` the turn sends, `None` for none (Python's
/// `turn_effort`): the turn's effort as the nearest level the model takes,
/// `minimal` as `low`; the model's lowest level where the turn states off
/// and the model cannot stop reasoning; none for a model that refuses the
/// parameter, and for an effort outside Anthropic's scale.
pub(crate) fn effort(request: &AiRequest, entry: Option<&ModelInfo>) -> Option<&'static str> {
    if tagged(entry, EFFORT_REFUSED) {
        return None;
    }
    let off = switch(request) == Some(false);
    let stated = request.reasoning_effort.as_deref();
    // The model's least effort stands for off, and for the effort `none`.
    let floor = reasoning_floor(entry).filter(|_| off || stated == Some("none"));
    let rank = SCALE
        .iter()
        .position(|level| floor.or(stated) == Some(*level))?;
    let up_to_high =
        tagged(entry, EFFORT_UP_TO_HIGH) || (off && tagged(entry, THINKING_OFF_EFFORT_HIGH));
    let taken = |level: &str| match level {
        "minimal" => false,
        "xhigh" => !up_to_high && !tagged(entry, EFFORT_NO_XHIGH),
        "max" => !up_to_high,
        _ => true,
    };
    // The nearest level the model takes, the lower one on a tie.
    let levels = SCALE.iter().enumerate().filter(|(_, level)| taken(level));
    let nearest = levels.min_by_key(|(at, _)| (at.abs_diff(rank), *at));
    nearest.map(|(_, level)| *level)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{entry, models};

    fn asking(effort: &str) -> AiRequest {
        AiRequest {
            reasoning_effort: Some(effort.to_owned()),
            ..AiRequest::default()
        }
    }

    // Python: tests/test_providers/test_anthropic.py::TestAnthropicEffort
    #[test]
    fn an_effort_goes_as_the_nearest_level_the_model_takes() {
        let sent = |model: &str, effort: &str| super::effort(&asking(effort), entry(model));
        assert_eq!(sent("claude-opus-5-5", "minimal"), Some("low"));
        assert_eq!(sent("claude-opus-5-5", "xhigh"), Some("xhigh"));
        assert_eq!(sent("claude-opus-5-5", "max"), Some("max"));
        // A tie goes to the lower level: `xhigh` between `high` and `max`.
        assert_eq!(sent("claude-sonnet-4-6", "xhigh"), Some("high"));
        assert_eq!(sent("claude-opus-4-5", "max"), Some("high"));
        assert_eq!(sent("claude-sonnet-4-5", "high"), None);
        assert_eq!(sent("no-such-model", "max"), Some("max"));
        for unknown in ["extreme", "HIGH", ""] {
            assert_eq!(sent("claude-opus-5-5", unknown), None, "{unknown}");
        }
        // `none` states off: a model that cannot stop is sent its lowest
        // level, and a model the catalogue does not know nothing.
        assert_eq!(sent("claude-opus-5-5", "none"), Some("low"));
        assert_eq!(sent("no-such-model", "none"), None);
    }

    /// A turn that states this budget, this switch and this effort.
    fn turn(budget: Option<i64>, enabled: Option<bool>, effort: Option<&str>) -> AiRequest {
        AiRequest {
            thinking_budget: budget,
            enable_thinking: enabled,
            reasoning_effort: effort.map(str::to_owned),
            ..AiRequest::default()
        }
    }

    // Python: src/roomkit/providers/anthropic/reasoning.py::thinking_or_temperature,
    // which sends `_thinking_off(model)` where the switch is False, and
    // nowhere else.
    // The block that turns thinking off goes to a turn that states off, of
    // a model whose catalogue entry declares one, and to no other turn: one
    // that says nothing of thinking sends no block, and one that thinks
    // sends the block of its thinking.
    #[test]
    fn the_block_that_turns_thinking_off_goes_to_a_turn_that_states_off_alone() {
        let declaring = [
            ("claude-opus-5", "disabled"),
            ("claude-sonnet-5", "disabled"),
            ("claude-haiku-5-5", "disabled"),
            ("claude-sonnet-5-5", "between_tools"),
        ];
        let declared = |model: &&ModelInfo| {
            let tags = OFF_BLOCKS.iter();
            tags.filter(|(tag, _)| tagged(Some(model), tag)).count()
        };
        // Every model of the catalogue that declares a block, each one block.
        let blocks: Vec<usize> = models().iter().map(|model| declared(&model)).collect();
        assert_eq!(blocks.iter().sum::<usize>(), declaring.len());
        assert!(blocks.iter().all(|declared| *declared <= 1));
        for (model, kind) in declaring {
            let adaptive = AnthropicConfig::new("placeholder-not-a-key", model);
            let budgeted = AnthropicConfig {
                use_adaptive_thinking: Some(false),
                ..adaptive.clone()
            };
            let sent = |config: &AnthropicConfig, turn: &AiRequest| {
                sampling(config, turn, entry(model)).thinking
            };
            for config in [&adaptive, &budgeted] {
                for off in [
                    turn(None, Some(false), None),
                    turn(Some(0), Some(true), None),
                    turn(Some(-1), None, None),
                    turn(None, None, Some("none")),
                ] {
                    assert_eq!(sent(config, &off), Some(json!({"type": kind})), "{model}");
                }
                for unsaid in [turn(None, None, None), turn(None, None, Some("high"))] {
                    assert_eq!(sent(config, &unsaid), None, "{model}");
                }
            }
            let thinks = json!({"type": "adaptive", "display": "summarized"});
            let on = turn(None, Some(true), None);
            assert_eq!(sent(&adaptive, &on), Some(thinks), "{model}");
            // A turn that does not think for want of a budget did not state
            // off.
            assert_eq!(sent(&budgeted, &on), None, "{model}");
            let on_a_budget = json!({"type": "enabled", "budget_tokens": 2048});
            let asked = turn(Some(2048), None, None);
            assert_eq!(sent(&budgeted, &asked), Some(on_a_budget), "{model}");
        }
        // A model that declares no block is sent none, whatever its turn
        // states.
        for model in ["claude-opus-5-5", "claude-opus-4-8", "no-such-model"] {
            let config = AnthropicConfig::new("placeholder-not-a-key", model);
            let off = turn(None, Some(false), None);
            assert_eq!(sampling(&config, &off, entry(model)).thinking, None);
        }
    }

    #[test]
    fn the_budget_outranks_the_switch_and_the_switch_the_effort() {
        let request =
            |budget: Option<i64>, enabled: Option<bool>, effort: Option<&str>| AiRequest {
                thinking_budget: budget,
                enable_thinking: enabled,
                reasoning_effort: effort.map(str::to_owned),
                ..AiRequest::default()
            };
        assert_eq!(switch(&request(Some(0), Some(true), None)), Some(false));
        assert_eq!(switch(&request(Some(-1), None, None)), Some(false));
        assert_eq!(switch(&request(Some(1), Some(false), None)), Some(true));
        assert_eq!(switch(&request(None, Some(true), Some("none"))), Some(true));
        assert_eq!(switch(&request(None, None, Some("none"))), Some(false));
        assert_eq!(switch(&request(None, None, Some("low"))), None);
    }
}
