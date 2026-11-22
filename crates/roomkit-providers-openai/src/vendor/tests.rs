//! The vendors' rules, as Python's providers state them.

use roomkit_ai::{AiMessage, AiRole, AiTool};
use serde_json::json;

use super::*;
use crate::{OpenAi, OpenAiConfig};

/// Whether `vendor`'s provider at `base_url` refuses, before any request, a
/// tool whose name holds a dot, which the rule of each vendor that has one
/// refuses.
fn checks_names(vendor: Vendor, base_url: &str) -> bool {
    let config = OpenAiConfig {
        base_url: Some(base_url.to_owned()),
        ..OpenAiConfig::new("placeholder-not-a-key", "a-model")
    };
    let provider = OpenAi::for_vendor(config, vendor).unwrap();
    let request = AiContext {
        tools: vec![AiTool::new("a.b", "A tool.").unwrap()],
        ..hello()
    };
    provider.asked().body(&request, false).is_err()
}

fn hello() -> AiContext {
    AiContext {
        messages: vec![AiMessage::new(AiRole::User, "Hi")],
        ..AiContext::default()
    }
}

// Python: tests/test_providers/test_vendor_endpoint.py::test_the_vendor_s_url_written_out_gets_the_vendor_s_rules
#[test]
fn a_vendor_s_rule_holds_on_its_own_endpoint_written_out_alone() {
    let kimi = |url: &str| checks_names(Vendor::Kimi, url);
    let deepseek = |url: &str| {
        let vendor = Vendor::DeepSeek {
            enable_thinking: None,
        };
        checks_names(vendor, url)
    };
    assert_eq!(
        [
            kimi("https://api.moonshot.ai/v1"),
            kimi("https://api.moonshot.ai/v1/"),
            kimi("http://gpu:8000/v1"),
            deepseek("https://api.deepseek.com/v1"),
            deepseek("https://api.deepseek.com"),
            deepseek("https://user:key@api.deepseek.com/v1"),
            checks_names(Vendor::MiniMax, "https://api.minimax.io/v1"),
        ],
        [true, true, false, true, true, false, false]
    );
}

// Python: tests/test_providers/test_reasoning_settings.py, nearest_level
#[test]
fn an_effort_goes_as_the_nearest_level_the_model_takes() {
    let kimi = |effort| nearest_level(Some(effort), &KIMI_LEVELS, &SCALE);
    assert_eq!(
        [
            kimi("medium"),
            kimi("xhigh"),
            kimi("minimal"),
            kimi("max"),
            kimi("none")
        ],
        [Some("low"), Some("high"), Some("low"), Some("max"), None]
    );
}

fn request(effort: Option<&str>, switch: Option<bool>) -> AiContext {
    AiContext {
        reasoning_effort: effort.map(str::to_owned),
        enable_thinking: switch,
        ..hello()
    }
}

fn thinking(sampling: &Sampling) -> Option<Value> {
    sampling.extra.get("thinking").cloned()
}

// Python: tests/test_providers/test_deepseek.py::TestThinking
#[test]
fn deepseek_s_thinking_object_is_its_one_reasoning_setting() {
    let deepseek = |enable_thinking, configured, effort, switch| {
        let vendor = Vendor::DeepSeek { enable_thinking };
        let sampling = vendor.sampling(None, &request(effort, switch), configured);
        assert_eq!(
            sampling.effort, None,
            "a top-level reasoning_effort is never sent"
        );
        thinking(&sampling)
    };
    assert_eq!(
        [
            // Nothing stated: the model's default.
            deepseek(None, None, None, None),
            // The effort nests inside the object.
            deepseek(None, Some("high"), None, None),
            // The configured switch, either way.
            deepseek(Some(false), None, None, None),
            deepseek(Some(true), None, None, None),
            // The turn outranks the configuration.
            deepseek(Some(true), None, None, Some(false)),
            deepseek(None, Some("high"), Some("low"), None),
            // An effort of none states off.
            deepseek(None, None, Some("none"), None),
        ],
        [
            None,
            Some(json!({ "type": "enabled", "reasoning_effort": "high" })),
            Some(json!({ "type": "disabled" })),
            Some(json!({ "type": "enabled" })),
            Some(json!({ "type": "disabled" })),
            Some(json!({ "type": "enabled", "reasoning_effort": "low" })),
            Some(json!({ "type": "disabled" })),
        ]
    );
}

// Python: tests/test_providers/test_deepseek.py::TestThinking::test_zero_budget_disables
// and ::test_positive_budget_enables_without_a_size
#[test]
fn deepseek_reads_a_budget_as_a_switch_without_its_size() {
    let budgeted = |budget| {
        let request = AiContext {
            thinking_budget: Some(budget),
            ..hello()
        };
        let vendor = Vendor::DeepSeek {
            enable_thinking: None,
        };
        thinking(&vendor.sampling(None, &request, None))
    };
    assert_eq!(
        [budgeted(0), budgeted(2048)],
        [
            Some(json!({ "type": "disabled" })),
            Some(json!({ "type": "enabled" }))
        ]
    );
}
