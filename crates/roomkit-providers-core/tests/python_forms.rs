//! The JSON forms of the contract's types against the ones `RoomKit`'s Python
//! models give: `fixtures/python_forms.json`, written by
//! `scripts/generate_python_forms.py` (`make python-forms`).
//!
//! Each example holds the fields a caller gives a Python model (`given`) and
//! what the model dumps with `model_dump(mode="json")` (`form`).

use std::collections::BTreeSet;

use roomkit_providers_core::{
    AiContentPart, AiImagePart, AiMessage, AiMessageContent, AiRequest, AiRole, AiTextPart,
    AiThinkingPart, AiTool, AiToolCallPart, AiToolResultPart, ToolCallOutcome, ToolResult,
    ToolResultPart,
};
use serde::de::{DeserializeOwned, Error};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const FIXTURE: &str = include_str!("fixtures/python_forms.json");

#[derive(Deserialize)]
struct Fixture {
    examples: Vec<Example>,
}

#[derive(Deserialize)]
struct Example {
    model: String,
    name: String,
    given: Value,
    form: Value,
}

impl Example {
    /// Which example a failure is about.
    fn label(&self) -> String {
        format!("{} `{}`", self.model, self.name)
    }
}

/// The JSON text `T` writes once it has read `value`.
fn rewritten<T: DeserializeOwned + Serialize>(value: &Value) -> serde_json::Result<String> {
    serde_json::to_string(&T::deserialize(value)?)
}

/// The JSON text the Rust type of the Python `model` writes once it has
/// read `value`.
fn rewritten_as(model: &str, value: &Value) -> serde_json::Result<String> {
    match model {
        "AITextPart" => rewritten::<AiTextPart>(value),
        "AIImagePart" => rewritten::<AiImagePart>(value),
        "AIThinkingPart" => rewritten::<AiThinkingPart>(value),
        "AIToolCallPart" => rewritten::<AiToolCallPart>(value),
        "AIToolResultPart" => rewritten::<AiToolResultPart>(value),
        "AITool" => rewritten::<AiTool>(value),
        "AIMessage" => rewritten::<AiMessage>(value),
        // The request is the fields a provider reads of Python's context.
        "AIContext" => rewritten::<AiRequest>(value),
        other => Err(Error::custom(format!(
            "no Rust type for the Python model {other}"
        ))),
    }
}

/// Asserts that `written`, a type's JSON text, is `form`: the same value,
/// its members in the same order.
fn assert_is_form(written: &str, form: &Value, label: &str) {
    let value = serde_json::from_str::<Value>(written).ok();
    assert_eq!(value.as_ref(), Some(form), "{label}");
    assert_eq!(written, form.to_string(), "{label}: order of the members");
}

#[test]
fn each_python_form_reads_and_writes_back_unchanged() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    for example in &fixture.examples {
        let label = example.label();
        let written = rewritten_as(&example.model, &example.form)
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        assert_is_form(&written, &example.form, &label);
    }
}

#[test]
fn the_fields_a_caller_gives_read_as_the_form_python_dumps() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    for example in &fixture.examples {
        let label = format!("{}, from the given fields", example.label());
        let written = rewritten_as(&example.model, &example.given)
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        assert_is_form(&written, &example.form, &label);
    }
}

/// The types Python has no model for hold the same JSON: a message's content
/// and each of its parts, a tool's result and each of its parts, a role and
/// an outcome.
#[test]
fn a_form_s_members_read_and_write_back_unchanged() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    let check = |written: serde_json::Result<String>, form: &Value, label: &str| {
        let written = written.unwrap_or_else(|error| panic!("{label}: {error}"));
        assert_is_form(&written, form, label);
    };
    for example in &fixture.examples {
        let label = example.label();
        if example.model == "AIMessage" {
            let content = &example.form["content"];
            check(rewritten::<AiMessageContent>(content), content, &label);
            check(
                rewritten::<AiRole>(&example.form["role"]),
                &example.form["role"],
                &label,
            );
            for part in content.as_array().into_iter().flatten() {
                check(rewritten::<AiContentPart>(part), part, &label);
            }
        }
        if example.model == "AIToolResultPart" {
            let result = &example.form["result"];
            check(rewritten::<ToolResult>(result), result, &label);
            for part in result.as_array().into_iter().flatten() {
                check(rewritten::<ToolResultPart>(part), part, &label);
            }
            let outcome = &example.form["outcome"];
            check(
                rewritten::<Option<ToolCallOutcome>>(outcome),
                outcome,
                &label,
            );
        }
    }
}

/// The fixture holds what the tests above are for: every model, every part
/// among a message's content, both shapes of a content and of a result,
/// every role, every outcome, and what Python dumps under a request's
/// `api_key`.
#[test]
fn the_fixture_covers_every_model_and_every_shape() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    let strings = |values: Vec<&Value>| -> BTreeSet<String> {
        values
            .into_iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    let of = |model: &str| -> Vec<&Value> {
        fixture
            .examples
            .iter()
            .filter(|example| example.model == model)
            .map(|example| &example.form)
            .collect()
    };
    let named = |names: &[&str]| -> BTreeSet<String> {
        names.iter().map(|&name| name.to_owned()).collect()
    };

    let models: BTreeSet<String> = fixture
        .examples
        .iter()
        .map(|example| example.model.clone())
        .collect();
    assert_eq!(
        models,
        named(&[
            "AIContext",
            "AIImagePart",
            "AIMessage",
            "AITextPart",
            "AIThinkingPart",
            "AITool",
            "AIToolCallPart",
            "AIToolResultPart",
        ])
    );

    let messages = of("AIMessage");
    assert_eq!(
        strings(messages.iter().map(|message| &message["role"]).collect()),
        named(&["assistant", "system", "tool", "user"])
    );
    assert!(
        messages
            .iter()
            .any(|message| message["content"].is_string())
    );
    let parts: Vec<&Value> = messages
        .iter()
        .filter_map(|message| message["content"].as_array())
        .flatten()
        .collect();
    assert_eq!(
        strings(parts.iter().map(|part| &part["type"]).collect()),
        named(&["image", "text", "thinking", "tool_call", "tool_result"])
    );

    let results = of("AIToolResultPart");
    assert!(results.iter().any(|result| result["result"].is_string()));
    let result_parts: Vec<&Value> = results
        .iter()
        .filter_map(|result| result["result"].as_array())
        .flatten()
        .collect();
    assert_eq!(
        strings(result_parts.iter().map(|part| &part["type"]).collect()),
        named(&["image", "text"])
    );
    assert_eq!(
        strings(results.iter().map(|result| &result["outcome"]).collect()),
        named(&[
            "blocked",
            "cancelled",
            "failed",
            "refused",
            "served",
            "unserved"
        ])
    );
    assert!(results.iter().any(|result| result["outcome"].is_null()));

    // A credential, dumped masked; an empty string and a value that is no
    // string, dumped as they are.
    let requests = of("AIContext");
    let credentials: Vec<&Value> = requests
        .iter()
        .filter_map(|request| request["metadata"].get("api_key"))
        .collect();
    assert!(
        credentials
            .iter()
            .any(|key| key.as_str() == Some("**********"))
    );
    assert!(credentials.iter().any(|key| key.as_str() == Some("")));
    assert!(credentials.iter().any(|key| !key.is_string()));
}
