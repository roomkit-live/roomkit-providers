//! DeepSeek's provider: its catalogue and its usage.
//! Python: `tests/test_providers/test_deepseek.py`.

use roomkit_ai::AiProvider;
use serde_json::{Value, json};

use super::*;
use crate::answer::usage_of;

// Python: tests/test_providers/test_deepseek.py::TestCatalog
#[test]
fn the_catalogue_is_python_s_and_reads_no_image() {
    let models = DeepSeek::available_models();
    let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
    assert_eq!(ids, ["deepseek-v4-pro", "deepseek-v4-flash"]);
    assert!(
        models
            .iter()
            .all(|model| model.pricing.is_some() && model.context_window.is_some())
    );
    assert!(
        models
            .iter()
            .all(|model| model.supports_vision == Some(false))
    );
}

/// What DeepSeek's provider reads of `usage`.
fn usage(usage: &Value) -> Value {
    Value::Object(usage_of(usage, DeepSeekConfig::VENDOR))
}

// Python: tests/test_providers/test_deepseek.py::TestUsage
#[test]
fn cache_hits_are_not_billed_as_input() {
    assert_eq!(
        [
            usage(&json!({ "prompt_tokens": 1000, "completion_tokens": 50,
                "prompt_cache_hit_tokens": 960, "prompt_cache_miss_tokens": 40 })),
            // The miss counter derived when it is left out.
            usage(&json!({ "prompt_tokens": 1000, "completion_tokens": 50,
                "prompt_cache_hit_tokens": 960 })),
            // A zero hit reports no cache counter.
            usage(&json!({ "prompt_tokens": 100, "completion_tokens": 5,
                "prompt_cache_hit_tokens": 0, "prompt_cache_miss_tokens": 100 })),
            // OpenAI's shape, from a server that reports no hits.
            usage(&json!({ "prompt_tokens": 100, "completion_tokens": 5,
                "prompt_tokens_details": { "cached_tokens": 80 } })),
        ],
        [
            json!({ "input_tokens": 40, "output_tokens": 50, "cache_read_input_tokens": 960 }),
            json!({ "input_tokens": 40, "output_tokens": 50, "cache_read_input_tokens": 960 }),
            json!({ "input_tokens": 100, "output_tokens": 5 }),
            json!({ "input_tokens": 20, "output_tokens": 5, "cache_read_input_tokens": 80 }),
        ]
    );
}

// Python: tests/test_providers/test_deepseek.py::TestDeepSeekUsage::test_reasoning_tokens_are_exposed_as_a_detail_of_output
#[test]
fn reasoning_tokens_are_exposed_as_a_detail_of_output() {
    assert_eq!(
        usage(&json!({ "prompt_tokens": 100, "completion_tokens": 500,
            "prompt_cache_hit_tokens": 0, "prompt_cache_miss_tokens": 100,
            "completion_tokens_details": { "reasoning_tokens": 450 } })),
        json!({ "input_tokens": 100, "output_tokens": 500, "reasoning_tokens": 450 })
    );
}

// Python: tests/test_providers/test_deepseek.py::TestDeepSeekUsage::test_a_count_the_server_leaves_out_stays_unknown
#[test]
fn a_count_the_server_leaves_out_stays_unknown() {
    assert_eq!(
        [
            usage(&json!({ "prompt_cache_hit_tokens": 960, "prompt_cache_miss_tokens": 40 })),
            usage(&json!({ "completion_tokens": 50, "prompt_cache_hit_tokens": 600 })),
        ],
        [
            json!({ "input_tokens": 40, "cache_read_input_tokens": 960 }),
            json!({ "output_tokens": 50, "cache_read_input_tokens": 600 }),
        ]
    );
}

// Python: OpenAIAIProvider reads only OpenAI's shape: hits a generic server
// reports are input there.
#[test]
fn a_generic_provider_reads_no_cache_hits() {
    let reported =
        json!({ "prompt_tokens": 1000, "completion_tokens": 50, "prompt_cache_hit_tokens": 960 });
    assert_eq!(
        Value::Object(usage_of(&reported, Vendor::OpenAi)),
        json!({ "input_tokens": 1000, "output_tokens": 50 })
    );
}

// Python: tests/test_providers/test_deepseek.py::TestDeepSeekProvider::test_provider_name_is_deepseek
#[test]
fn an_error_names_deepseek() {
    let config = DeepSeekConfig::new("placeholder-not-a-key", "deepseek-v4-pro");
    let provider = DeepSeek::new(config).unwrap();
    assert_eq!(
        (provider.name(), provider.label()),
        ("DeepSeekAIProvider", "deepseek")
    );
}
