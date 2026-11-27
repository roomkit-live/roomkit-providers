//! The configurations of the vendors that speak `OpenAI`'s wire against the
//! JSON forms of their `RoomKit` Python classes, derivatives of
//! `OpenAIConfig`: `tests/fixtures/python_<vendor>_config_forms.json`,
//! written by `scripts/generate_openai_config_forms.py`
//! (`make python-openai-config`).

use roomkit_providers_openai::{
    CerebrasConfig, DeepSeekConfig, KimiConfig, MetaConfig, MiniMaxConfig, QwenConfig, XaiConfig,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize)]
struct Fixture {
    model: String,
    examples: Vec<Example>,
    rejected: Vec<Given>,
    converted: Vec<Example>,
}

/// What a configuration is built from, and what Python dumps of it.
#[derive(Deserialize)]
struct Example {
    name: String,
    given: Value,
    form: Value,
}

#[derive(Deserialize)]
struct Given {
    name: String,
    given: Value,
}

#[expect(
    clippy::unwrap_used,
    reason = "a test's helper: a fixture that does not read fails the test"
)]
fn fixture(text: &str) -> Fixture {
    serde_json::from_str(text).unwrap()
}

/// The JSON text a configuration of type `C` writes once it has read
/// `value`. A value and its JSON text are two readers: both must read alike.
fn rewritten<C: DeserializeOwned + Serialize>(value: &Value) -> serde_json::Result<String> {
    let write = |config: C| serde_json::to_string(&config);
    let from_value = C::deserialize(value).and_then(write);
    let from_text = serde_json::from_str::<C>(&value.to_string()).and_then(write);
    assert_eq!(
        from_value.as_ref().ok(),
        from_text.as_ref().ok(),
        "a value and its JSON text read alike: {value}"
    );
    from_value
}

/// Every part of `text`'s fixture, read and written with `C`, as Python
/// decided it: an example reads as the form Python dumps and the form as
/// itself, the key is masked, and what Python refuses or converts is
/// refused here.
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test's helper: a fixture that does not read fails the test"
)]
fn holds_to_python<C: DeserializeOwned + Serialize>(text: &str, model: &str, members: usize) {
    let fixture = fixture(text);
    assert_eq!(fixture.model, model);
    for example in &fixture.examples {
        let form = example.form.to_string();
        for read in [&example.given, &example.form] {
            let written = rewritten::<C>(read)
                .unwrap_or_else(|error| panic!("{model}, {}: {error}", example.name));
            assert_eq!(written, form, "{model}, {}", example.name);
        }
        let key = example.given["api_key"].as_str().unwrap();
        let mask = if key.is_empty() { "" } else { "**********" };
        assert_eq!(example.form["api_key"], mask, "{model}, {}", example.name);
    }
    for rejected in &fixture.rejected {
        assert!(
            rewritten::<C>(&rejected.given).is_err(),
            "{model}, {}",
            rejected.name
        );
    }
    for converted in &fixture.converted {
        assert!(converted.form.is_object(), "{model}, {}", converted.name);
        assert!(
            rewritten::<C>(&converted.given).is_err(),
            "{model}, {}",
            converted.name
        );
    }
    // A regenerated fixture that lost a kind of example fails here: every
    // member is stated otherwise than its default by some example, but the
    // two the vendor's rule leaves unset.
    let defaults = &fixture.examples[0].form;
    let named: Vec<&String> = defaults.as_object().unwrap().keys().collect();
    assert_eq!(named.len(), members, "{model}");
    let moot = ["supports_reasoning_effort_with_tools", "thinking_field"];
    for member in named
        .into_iter()
        .filter(|member| !moot.contains(&member.as_str()))
    {
        let stated = fixture
            .examples
            .iter()
            .any(|example| example.form[member] != defaults[member]);
        assert!(stated, "{model}: no example states {member}");
    }
    // A vendor that names a model of its own takes a form without one, and
    // one that takes a thinking field refuses no thinking field: 27 at least.
    assert!(
        fixture.rejected.len() >= 27 && fixture.converted.len() >= 11,
        "{model}"
    );
}

#[test]
fn deepseek_s_configuration_reads_and_writes_as_python_s() {
    holds_to_python::<DeepSeekConfig>(
        include_str!("fixtures/python_deepseek_config_forms.json"),
        "DeepSeekConfig",
        18,
    );
}

#[test]
fn kimi_s_configuration_reads_and_writes_as_python_s() {
    holds_to_python::<KimiConfig>(
        include_str!("fixtures/python_kimi_config_forms.json"),
        "KimiConfig",
        17,
    );
}

#[test]
fn minimax_s_configuration_reads_and_writes_as_python_s() {
    holds_to_python::<MiniMaxConfig>(
        include_str!("fixtures/python_minimax_config_forms.json"),
        "MiniMaxConfig",
        17,
    );
}

#[test]
fn xai_s_configuration_reads_and_writes_as_python_s() {
    holds_to_python::<XaiConfig>(
        include_str!("fixtures/python_xai_config_forms.json"),
        "XAIConfig",
        17,
    );
}

#[test]
fn cerebras_s_configuration_reads_and_writes_as_python_s() {
    holds_to_python::<CerebrasConfig>(
        include_str!("fixtures/python_cerebras_config_forms.json"),
        "CerebrasConfig",
        19,
    );
}

#[test]
fn qwen_s_configuration_reads_and_writes_as_python_s() {
    holds_to_python::<QwenConfig>(
        include_str!("fixtures/python_qwen_config_forms.json"),
        "QwenConfig",
        18,
    );
}

#[test]
fn meta_s_configuration_reads_and_writes_as_python_s() {
    holds_to_python::<MetaConfig>(
        include_str!("fixtures/python_meta_config_forms.json"),
        "MetaConfig",
        17,
    );
}
