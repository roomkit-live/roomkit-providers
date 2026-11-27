//! The configuration against the JSON forms of `RoomKit`'s Python
//! `GeminiConfig`: `tests/fixtures/python_config_forms.json`, written by
//! `scripts/generate_gemini_config_forms.py`.

use roomkit_providers_gemini::GeminiConfig;
use serde::Deserialize;
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

/// The JSON text a configuration writes once it has read `value`. A value
/// and its JSON text are two readers: both must read alike.
fn rewritten(value: &Value) -> serde_json::Result<String> {
    let write = |config: GeminiConfig| serde_json::to_string(&config);
    let from_value = GeminiConfig::deserialize(value).and_then(write);
    let from_text = serde_json::from_str::<GeminiConfig>(&value.to_string()).and_then(write);
    assert_eq!(
        from_value.as_ref().ok(),
        from_text.as_ref().ok(),
        "a value and its JSON text read alike: {value}"
    );
    from_value
}

// Every example reads as the form Python dumps and the form as itself, the
// key masked; what Python refuses or converts is refused here.
#[test]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test: a fixture that does not read fails it"
)]
fn the_configuration_reads_and_writes_as_python_s() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/python_config_forms.json")).unwrap();
    assert_eq!(fixture.model, "GeminiConfig");
    for example in &fixture.examples {
        let form = example.form.to_string();
        for read in [&example.given, &example.form] {
            let written =
                rewritten(read).unwrap_or_else(|error| panic!("{}: {error}", example.name));
            assert_eq!(written, form, "{}", example.name);
        }
        let key = example.given["api_key"].as_str().unwrap();
        let mask = if key.is_empty() { "" } else { "**********" };
        assert_eq!(example.form["api_key"], mask, "{}", example.name);
    }
    for rejected in &fixture.rejected {
        assert!(rewritten(&rejected.given).is_err(), "{}", rejected.name);
    }
    for converted in &fixture.converted {
        assert!(rewritten(&converted.given).is_err(), "{}", converted.name);
    }
    assert!(fixture.examples.len() >= 11 && fixture.rejected.len() >= 11);
    assert!(fixture.converted.len() >= 4);
}
