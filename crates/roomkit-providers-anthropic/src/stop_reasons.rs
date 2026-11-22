//! Anthropic's stop reasons in the vocabulary of the provider contract
//! (RFC §6.7): Python's `providers/anthropic/stop_reasons.py`.

use roomkit_ai::FinishReason;

/// A message's stop reason in the contract's vocabulary: `None` for none,
/// and for one with no word there (`pause_turn`, a turn the server paused
/// and that has not ended).
pub(crate) fn finish_reason_of(stop_reason: Option<&str>) -> Option<FinishReason> {
    match stop_reason? {
        "end_turn" | "stop_sequence" => Some(FinishReason::Stop),
        "tool_use" => Some(FinishReason::ToolCalls),
        "max_tokens" | "model_context_window_exceeded" => Some(FinishReason::Length),
        "refusal" => Some(FinishReason::Refusal),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Python: tests/test_providers/test_anthropic.py::test_a_stop_reason_ends_in_the_contract_s_word
    #[test]
    fn a_stop_reason_ends_in_the_contract_s_word() {
        let read = [
            (Some("end_turn"), Some(FinishReason::Stop)),
            (Some("stop_sequence"), Some(FinishReason::Stop)),
            (Some("tool_use"), Some(FinishReason::ToolCalls)),
            (Some("max_tokens"), Some(FinishReason::Length)),
            (
                Some("model_context_window_exceeded"),
                Some(FinishReason::Length),
            ),
            (Some("refusal"), Some(FinishReason::Refusal)),
            (Some("pause_turn"), None),
            (None, None),
        ];
        for (stop_reason, reason) in read {
            assert_eq!(finish_reason_of(stop_reason), reason, "{stop_reason:?}");
        }
    }
}
