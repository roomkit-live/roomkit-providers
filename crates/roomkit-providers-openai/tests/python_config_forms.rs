//! `OpenAiConfig` against the JSON forms of `RoomKit`'s Python
//! `OpenAIConfig`: `tests/fixtures/python_config_forms.json`, written by
//! `scripts/generate_openai_config_forms.py` (`make python-openai-config`).

use std::collections::BTreeMap;

use roomkit_providers_openai::OpenAiConfig;
use serde::Deserialize;
use serde_json::{Value, json};

const FIXTURE: &str = include_str!("fixtures/python_config_forms.json");

#[derive(Deserialize)]
struct Fixture {
    model: String,
    examples: Vec<Example>,
    rejected: Vec<Given>,
    converted: Vec<Example>,
    endpoints: Vec<Endpoint>,
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

/// A base URL, and whether it is `OpenAI`'s own endpoint for Python: `true`,
/// `false`, or `"refused"` when Python's URL reader refuses it.
#[derive(Deserialize)]
struct Endpoint {
    base_url: String,
    openai: Value,
}

/// The base URLs Python's URL reader refuses, and the configuration with
/// them: a bracket that holds no IPv6 address or is not closed, and a
/// character that Unicode's compatibility forms turn into a delimiter. Here
/// a configuration is a plain value, built without a check: each is read,
/// and is not `OpenAI`'s endpoint.
const REFUSED_BY_PYTHON: [&str; 4] = [
    "https://[api.openai.com]/v1",
    "https://[::1/v1",
    "https://api.openai.com]/v1",
    "https://api.openai.com\u{ff0f}v1",
];

#[expect(
    clippy::unwrap_used,
    reason = "a test's helper: a fixture that does not read fails the test"
)]
fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).unwrap()
}

/// The JSON text a configuration writes once it has read `value`. A value
/// and its JSON text are two readers: both must read alike.
fn rewritten(value: &Value) -> serde_json::Result<String> {
    let write = |config: OpenAiConfig| serde_json::to_string(&config);
    let from_value = OpenAiConfig::deserialize(value).and_then(write);
    let from_text = serde_json::from_str::<OpenAiConfig>(&value.to_string()).and_then(write);
    assert_eq!(
        from_value.as_ref().ok(),
        from_text.as_ref().ok(),
        "a value and its JSON text read alike: {value}"
    );
    from_value
}

#[test]
fn what_a_caller_gives_reads_as_the_form_python_dumps_and_the_form_as_itself() {
    let fixture = fixture();
    assert_eq!(fixture.model, "OpenAIConfig");
    for example in &fixture.examples {
        let form = example.form.to_string();
        for read in [&example.given, &example.form] {
            let written = rewritten(read).unwrap_or_else(|error| {
                panic!("{}: {error}", example.name);
            });
            assert_eq!(written, form, "{}", example.name);
        }
    }
}

#[test]
fn the_key_is_written_masked_and_an_empty_one_empty() {
    let fixture = fixture();
    for example in &fixture.examples {
        let key = example.given["api_key"].as_str().unwrap();
        let mask = if key.is_empty() { "" } else { "**********" };
        assert_eq!(example.form["api_key"], mask, "{}", example.name);
        let config = OpenAiConfig::deserialize(&example.given).unwrap();
        assert_eq!(config.api_key, key, "{}", example.name);
        let written = serde_json::to_string(&config).unwrap();
        assert!(key.is_empty() || !written.contains(key), "{}", example.name);
    }
}

#[test]
fn the_inputs_python_refuses_are_refused() {
    let fixture = fixture();
    for rejected in &fixture.rejected {
        assert!(rewritten(&rejected.given).is_err(), "{}", rejected.name);
    }
}

/// Input is stricter than Python's: what pydantic converts, and a number
/// outside its Rust type, is refused here.
#[test]
fn the_inputs_python_converts_are_refused_here() {
    let fixture = fixture();
    for converted in &fixture.converted {
        assert!(converted.form.is_object(), "{}", converted.name);
        assert!(rewritten(&converted.given).is_err(), "{}", converted.name);
    }
}

/// A modern model takes its defaults on `OpenAI`'s own endpoint, however
/// its base URL is written, and nowhere else.
#[test]
fn a_base_url_is_openai_s_endpoint_where_python_says_so() {
    let fixture = fixture();
    let mut refused = Vec::new();
    for endpoint in &fixture.endpoints {
        let config = OpenAiConfig {
            base_url: Some(endpoint.base_url.clone()),
            ..OpenAiConfig::new("placeholder-not-a-key", "gpt-5.1")
        };
        let written = serde_json::to_value(&config).unwrap();
        let profiled = written["use_max_completion_tokens"] == true;
        assert_eq!(
            written["supports_custom_temperature"], !profiled,
            "{:?}",
            endpoint.base_url
        );
        let python = endpoint.openai.as_bool().unwrap_or_else(|| {
            assert_eq!(endpoint.openai, "refused", "{:?}", endpoint.base_url);
            refused.push(endpoint.base_url.as_str());
            false
        });
        assert_eq!(profiled, python, "{:?}", endpoint.base_url);
    }
    assert_eq!(refused, REFUSED_BY_PYTHON);
}

/// A regenerated fixture that lost a kind of example fails here.
#[test]
fn the_fixture_covers_every_member_and_every_decision() {
    let fixture = fixture();
    let defaults = &fixture.examples[0].form;
    let members: Vec<&String> = defaults.as_object().unwrap().keys().collect();
    assert_eq!(members.len(), 17);
    for member in members {
        let stated = fixture
            .examples
            .iter()
            .any(|example| example.form[member] != defaults[member]);
        assert!(stated, "no example states {member}");
    }
    let own = |endpoint: &&Endpoint| endpoint.openai == true;
    assert!(fixture.endpoints.iter().filter(own).count() >= 20);
    assert!(fixture.endpoints.len() >= 65);
    assert!(fixture.rejected.len() >= 25 && fixture.converted.len() >= 11);
}

/// What the form does not carry, and what it orders otherwise than Python.
#[test]
fn vision_is_no_part_of_the_form_and_headers_are_written_sorted() {
    let config = OpenAiConfig {
        vision: true,
        default_headers: Some(BTreeMap::from([
            ("X-B".to_owned(), "2".to_owned()),
            ("X-A".to_owned(), "1".to_owned()),
        ])),
        ..OpenAiConfig::new("placeholder-not-a-key", "gpt-4o")
    };
    let written = serde_json::to_string(&config).unwrap();
    assert!(!written.contains("vision"));
    assert!(written.contains(r#""default_headers":{"X-A":"1","X-B":"2"}"#));
    let read: OpenAiConfig = serde_json::from_value(json!({
        "api_key": "placeholder-not-a-key",
        "model": "gpt-4o",
        "vision": true,
        "default_headers": {"X-B": "2", "X-A": "1"},
    }))
    .unwrap();
    assert!(!read.vision);
    assert_eq!(
        read,
        OpenAiConfig {
            vision: false,
            ..config
        }
    );
}
