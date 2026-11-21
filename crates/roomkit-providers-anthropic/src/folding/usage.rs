//! The counters of a streamed message: what its start tells of its input,
//! and what its end tells of the whole (Python's `input_usage` and
//! `done_event`).
//!
//! A count is read when it is a JSON whole number, a float with no fraction
//! included. Any other (a text, `true`, a fraction, a number below zero)
//! stays unknown: it is neither converted nor fatal.

use roomkit_providers_llm::Metadata;
use serde_json::Value;

/// The counters of one message, each unknown until it is told.
#[derive(Debug, Default)]
pub(super) struct Usage {
    input: Option<u64>,
    output: Option<u64>,
    cache_write: Option<u64>,
    cache_read: Option<u64>,
    reasoning: Option<u64>,
}

/// A count of tokens a server wrote as a whole number, a float with no
/// fraction included; `None` for anything else, which stays unknown.
fn count(value: Option<&Value>) -> Option<u64> {
    let number = value?.as_number()?;
    number.as_u64().or_else(|| {
        let float = number
            .as_f64()
            .filter(|float| *float >= 0.0 && float.fract() == 0.0)?;
        format!("{float:.0}").parse().ok()
    })
}

/// `held` with `told` in its place, when `told` reads as a count.
fn replaced(held: &mut Option<u64>, told: Option<&Value>) {
    if let Some(told) = count(told) {
        *held = Some(told);
    }
}

impl Usage {
    /// Takes what the start of a message tells: its input, and its cache
    /// reads and writes. The output it tells then is no count yet.
    pub(super) fn start(&mut self, told: Option<&Value>) {
        let Some(told) = told else {
            return;
        };
        replaced(&mut self.input, told.get("input_tokens"));
        self.told_of_the_cache(told);
    }

    /// Takes what the end of a message tells: its output, and each counter
    /// it tells again, which replaces what the start told.
    pub(super) fn end(&mut self, told: Option<&Value>) {
        let Some(told) = told else {
            return;
        };
        replaced(&mut self.input, told.get("input_tokens"));
        replaced(&mut self.output, told.get("output_tokens"));
        self.told_of_the_cache(told);
    }

    /// Takes the cache counters `told` holds, and the reasoning tokens,
    /// which are a detail of the output tokens.
    fn told_of_the_cache(&mut self, told: &Value) {
        replaced(
            &mut self.cache_write,
            told.get("cache_creation_input_tokens"),
        );
        replaced(&mut self.cache_read, told.get("cache_read_input_tokens"));
        let details = told.get("output_tokens_details");
        let thinking = details.and_then(|details| details.get("thinking_tokens"));
        replaced(&mut self.reasoning, thinking);
    }

    /// The counters with their names, a cache counter and the reasoning
    /// tokens left out at zero, as every provider leaves them out.
    fn named(&self, with_the_output: bool) -> Metadata {
        let above_zero = |counter: Option<u64>| counter.filter(|counter| *counter > 0);
        let output = self.output.filter(|_| with_the_output);
        let reasoning = above_zero(self.reasoning).filter(|_| with_the_output);
        let counters = [
            ("input_tokens", self.input),
            ("output_tokens", output),
            ("cache_creation_input_tokens", above_zero(self.cache_write)),
            ("cache_read_input_tokens", above_zero(self.cache_read)),
            ("reasoning_tokens", reasoning),
        ];
        let known = counters.into_iter().filter_map(|(name, counter)| {
            let counter = counter?;
            Some((name.to_owned(), Value::from(counter)))
        });
        known.collect()
    }

    /// What a message's start told of its input, for a generation cut
    /// before its end to count: empty when it told nothing.
    pub(super) fn of_the_input(&self) -> Metadata {
        self.named(false)
    }

    /// The usage of the whole message.
    pub(super) fn whole(&self) -> Metadata {
        self.named(true)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_count_is_a_whole_number_and_nothing_else() {
        let read = |text: &str| count(serde_json::from_str::<Value>(text).ok().as_ref());
        assert_eq!(read("12"), Some(12));
        assert_eq!(read("12.0"), Some(12));
        assert_eq!(read("0"), Some(0));
        for unknown in ["12.5", "-12", "\"12\"", "true", "null", "[12]", "1e400"] {
            assert_eq!(read(unknown), None, "{unknown}");
        }
    }

    // Source: src/tests.rs::a_message_start_tells_its_input
    // Python: tests/test_providers/test_anthropic.py::TestAnthropicUsageCounters::test_a_message_start_tells_its_input
    #[test]
    fn a_message_start_tells_its_input_and_its_cache_counters_above_zero() {
        let told = |start: Value| {
            let mut usage = Usage::default();
            usage.start(Some(&start));
            Value::Object(usage.of_the_input())
        };
        assert_eq!(
            told(json!({"input_tokens": 11, "output_tokens": 7})),
            json!({"input_tokens": 11})
        );
        let cached = json!({
            "input_tokens": 11,
            "output_tokens": 7,
            "cache_creation_input_tokens": 5,
            "cache_read_input_tokens": 3,
        });
        let expected = json!({
            "input_tokens": 11,
            "cache_creation_input_tokens": 5,
            "cache_read_input_tokens": 3,
        });
        assert_eq!(told(cached).to_string(), expected.to_string());
        let zero = json!({"input_tokens": 0, "cache_read_input_tokens": 0});
        assert_eq!(told(zero), json!({"input_tokens": 0}));
        assert_eq!(told(json!({})), json!({}));
    }

    #[test]
    fn what_the_end_tells_replaces_what_the_start_told() {
        let mut usage = Usage::default();
        usage.start(Some(&json!({
            "input_tokens": 12,
            "cache_creation_input_tokens": 512,
            "cache_read_input_tokens": 8,
            "output_tokens": 1,
        })));
        // The output the start tells is no count.
        assert_eq!(usage.whole().get("output_tokens"), None);
        usage.end(Some(&json!({
            "input_tokens": 40,
            "cache_creation_input_tokens": 0,
            "cache_read_input_tokens": null,
            "output_tokens": 9,
            "output_tokens_details": {"thinking_tokens": 4},
        })));
        let expected = json!({
            "input_tokens": 40,
            "output_tokens": 9,
            "cache_read_input_tokens": 8,
            "reasoning_tokens": 4,
        });
        assert_eq!(
            Value::Object(usage.whole()).to_string(),
            expected.to_string()
        );
    }
}
