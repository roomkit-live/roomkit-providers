//! The Vertex AI configuration against the JSON forms of `RoomKit`'s Python
//! `GeminiVertexConfig`: `tests/fixtures/python_vertex_config_forms.json`,
//! written by `scripts/generate_vertex_config_forms.py`. Its billing labels
//! are held to Google's rules as Python holds them.

use roomkit_providers_gemini::GeminiVertexConfig;
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
    let write = |config: GeminiVertexConfig| serde_json::to_string(&config);
    let from_value = GeminiVertexConfig::deserialize(value).and_then(write);
    let from_text = serde_json::from_str::<GeminiVertexConfig>(&value.to_string()).and_then(write);
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
fn the_configuration_reads_and_writes_as_python_s() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/python_vertex_config_forms.json")).unwrap();
    assert_eq!(fixture.model, "GeminiVertexConfig");
    for example in &fixture.examples {
        let form = example.form.to_string();
        for read in [&example.given, &example.form] {
            let written =
                rewritten(read).unwrap_or_else(|error| panic!("{}: {error}", example.name));
            assert_eq!(written, form, "{}", example.name);
        }
        for secret in ["api_key", "service_account_json"] {
            let given = example.given.get(secret).and_then(|given| given.as_str());
            let mask = given.map(|given| if given.is_empty() { "" } else { "**********" });
            assert_eq!(example.form[secret].as_str(), mask, "{}", example.name);
        }
    }
    for rejected in &fixture.rejected {
        assert!(rewritten(&rejected.given).is_err(), "{}", rejected.name);
    }
    for converted in &fixture.converted {
        assert!(rewritten(&converted.given).is_err(), "{}", converted.name);
    }
    assert!(fixture.examples.len() >= 10 && fixture.rejected.len() >= 20);
    assert!(fixture.converted.len() >= 2);
}
