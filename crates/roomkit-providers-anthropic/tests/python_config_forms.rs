//! `AnthropicConfig` against the JSON forms of `RoomKit`'s Python
//! `AnthropicConfig`: `tests/fixtures/python_config_forms.json`, written by
//! `scripts/generate_anthropic_config_forms.py`.

use std::collections::BTreeMap;

use roomkit_providers_anthropic::AnthropicConfig;
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

/// A base URL, what Python's configuration keeps of it, and whether it is
/// Anthropic's own endpoint for Python: `true`, `false`, or `"refused"`
/// when Python's URL reader refuses it, and then keeps nothing.
#[derive(Deserialize)]
struct Endpoint {
    base_url: String,
    kept: Option<String>,
    anthropic: Value,
}

/// The base URLs Python's URL reader refuses, and the configuration with
/// them: a bracket that holds no IPv6 address or is not closed, and a
/// character that Unicode's compatibility forms turn into a delimiter. Here
/// a configuration is a plain value, built without a check: each is read,
/// and is not Anthropic's endpoint.
const REFUSED_BY_PYTHON: [&str; 4] = [
    "https://[api.anthropic.com]",
    "https://[::1",
    "https://api.anthropic.com]",
    "https://api.anthropic.com\u{ff0f}",
];

/// The one example whose headers Python writes in another order than their
/// names': the form here is written sorted.
const HEADERS_IN_ANOTHER_ORDER: &str = "headers";

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
    let write = |config: AnthropicConfig| serde_json::to_string(&config);
    let from_value = AnthropicConfig::deserialize(value).and_then(write);
    let from_text = serde_json::from_str::<AnthropicConfig>(&value.to_string()).and_then(write);
    assert_eq!(
        from_value.as_ref().ok(),
        from_text.as_ref().ok(),
        "a value and its JSON text read alike: {value}"
    );
    from_value
}

/// `form` with its extra headers sorted by name, as the configuration
/// writes them.
fn with_headers_sorted(form: &Value) -> Value {
    let mut form = form.clone();
    let headers = form.get_mut("extra_headers");
    if let Some(headers) = headers.and_then(Value::as_object_mut) {
        let sorted: BTreeMap<String, Value> = std::mem::take(headers).into_iter().collect();
        headers.extend(sorted);
    }
    form
}

#[test]
fn what_a_caller_gives_reads_as_the_form_python_dumps_and_the_form_as_itself() {
    let fixture = fixture();
    assert_eq!(fixture.model, "AnthropicConfig");
    let mut reordered = Vec::new();
    for example in &fixture.examples {
        let (sorted, dumped) = (with_headers_sorted(&example.form), &example.form);
        let (sorted, dumped) = (sorted.to_string(), dumped.to_string());
        if sorted != dumped {
            reordered.push(example.name.as_str());
        }
        for read in [&example.given, &example.form] {
            let written = rewritten(read).unwrap_or_else(|error| {
                panic!("{}: {error}", example.name);
            });
            assert_eq!(written, sorted, "{}", example.name);
        }
    }
    // The headers are the one thing written in another order than Python's.
    assert_eq!(reordered, [HEADERS_IN_ANOTHER_ORDER]);
}

#[test]
fn the_key_is_written_masked_and_an_empty_one_empty() {
    let fixture = fixture();
    for example in &fixture.examples {
        let key = example.given["api_key"].as_str().unwrap();
        let mask = if key.is_empty() { "" } else { "**********" };
        assert_eq!(example.form["api_key"], mask, "{}", example.name);
        let config = AnthropicConfig::deserialize(&example.given).unwrap();
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

/// A base URL is written as Python's configuration keeps it, read or built:
/// without the white space around it, the slashes that end it and the path
/// of the Messages API it ends with. A modern model takes its defaults on
/// Anthropic's own endpoint, however its base URL is written, and nowhere
/// else.
#[test]
fn a_base_url_is_kept_and_is_anthropic_s_endpoint_where_python_says_so() {
    let fixture = fixture();
    let mut refused = Vec::new();
    for endpoint in &fixture.endpoints {
        let url = &endpoint.base_url;
        let built = AnthropicConfig {
            base_url: Some(url.clone()),
            ..AnthropicConfig::new("placeholder-not-a-key", "claude-opus-5-5")
        };
        let given = json!({
            "api_key": "placeholder-not-a-key",
            "model": "claude-opus-5-5",
            "base_url": url,
        });
        let read = AnthropicConfig::deserialize(&given).unwrap();
        let written = serde_json::to_value(&built).unwrap();
        assert_eq!(written, serde_json::to_value(&read).unwrap(), "{url:?}");
        let profiled = written["use_adaptive_thinking"] == true;
        assert_eq!(written["supports_custom_temperature"], !profiled, "{url:?}");
        let python = endpoint.anthropic.as_bool().unwrap_or_else(|| {
            assert_eq!(endpoint.anthropic, "refused", "{url:?}");
            assert_eq!(endpoint.kept, None, "{url:?}");
            refused.push(url.as_str());
            false
        });
        assert_eq!(profiled, python, "{url:?}");
        if let Some(kept) = &endpoint.kept {
            assert_eq!(written["base_url"], *kept, "{url:?}");
        }
        // What was given stays what a configuration holds, read or built.
        assert_eq!(read, built, "{url:?}");
        assert_eq!(built.base_url.as_ref(), Some(url));
    }
    assert_eq!(refused, REFUSED_BY_PYTHON);
}

/// A regenerated fixture that lost a kind of example fails here.
#[test]
fn the_fixture_covers_every_member_and_every_decision() {
    let fixture = fixture();
    let defaults = &fixture.examples[0].form;
    let members: Vec<&String> = defaults.as_object().unwrap().keys().collect();
    assert_eq!(
        members,
        [
            "api_key",
            "model",
            "max_tokens",
            "timeout",
            "connect_timeout",
            "max_retries",
            "base_url",
            "extra_headers",
            "enable_prompt_caching",
            "use_adaptive_thinking",
            "supports_custom_temperature",
        ]
    );
    for member in members {
        let stated = fixture
            .examples
            .iter()
            .any(|example| example.form[member] != defaults[member]);
        assert!(stated, "no example states {member}");
    }
    let own = |endpoint: &&Endpoint| endpoint.anthropic == true;
    assert!(fixture.endpoints.iter().filter(own).count() >= 20);
    let kept_otherwise = |endpoint: &&Endpoint| {
        endpoint
            .kept
            .as_deref()
            .is_some_and(|kept| kept != endpoint.base_url)
    };
    assert!(fixture.endpoints.iter().filter(kept_otherwise).count() >= 15);
    assert!(fixture.endpoints.len() >= 73);
    assert!(fixture.examples.len() >= 59);
    assert!(fixture.rejected.len() >= 23 && fixture.converted.len() >= 11);
}

/// What the JSON parser reads otherwise than Python's, what the form does
/// not carry, and what it writes whole, as Python's does: only the key is
/// masked.
#[test]
fn a_member_twice_an_unknown_member_and_what_the_form_does_not_hide() {
    let twice = r#"{"api_key": "placeholder-not-a-key", "model": "a", "model": "b"}"#;
    assert!(serde_json::from_str::<AnthropicConfig>(twice).is_err());
    let unknown = json!({"api_key": "", "model": "a", "vision": true, "organization": "acme"});
    let config = AnthropicConfig::deserialize(&unknown).unwrap();
    assert_eq!(config, AnthropicConfig::new("", "a"));

    let config = AnthropicConfig {
        base_url: Some("https://user:placeholder-in-the-url@llm.example".to_owned()),
        extra_headers: Some(BTreeMap::from([
            ("X-B".to_owned(), "placeholder-in-a-header".to_owned()),
            ("X-A".to_owned(), "1".to_owned()),
        ])),
        ..AnthropicConfig::new("placeholder-not-a-key", "local-model")
    };
    let written = serde_json::to_string(&config).unwrap();
    assert!(written.contains("placeholder-in-the-url"));
    assert!(written.contains(r#""extra_headers":{"X-A":"1","X-B":"placeholder-in-a-header"}"#));
    assert!(!written.contains("placeholder-not-a-key"));
    let shown = format!("{config:?}");
    assert!(!shown.contains("placeholder"), "{shown}");
}
