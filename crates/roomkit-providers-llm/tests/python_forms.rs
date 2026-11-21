//! The JSON forms of the contract's types against the ones `RoomKit`'s Python
//! models give: `fixtures/python_forms.json`, written by
//! `scripts/generate_python_forms.py` (`make python-forms`).
//!
//! Each example holds the fields a caller gives a Python model (`given`) and
//! what the model dumps with `model_dump(mode="json")` (`form`). Each
//! refusal holds an input Python refuses, and the words of its `ValueError`;
//! each rejected input, one Python refuses in pydantic's own words. Each
//! long float holds a number Python writes with 16 or 17 digits, its exact
//! bits, and the JSON text of models that carry it.

use std::collections::BTreeSet;

use roomkit_providers_llm::{
    AiContentPart, AiImagePart, AiMessage, AiMessageContent, AiRequest, AiResponse, AiRole,
    AiTextPart, AiThinkingPart, AiTool, AiToolCall, AiToolCallPart, AiToolResultPart, Date,
    ModelInfo, ModelPricing, ServedCall, StreamDone, StreamEvent, StreamTextDelta,
    StreamThinkingDelta, StreamToolCall, StreamToolCallDelta, StreamUsage, ToolCallOutcome,
    ToolResult, ToolResultPart, tool_arguments,
};
use serde::de::{DeserializeOwned, Error};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const FIXTURE: &str = include_str!("fixtures/python_forms.json");

#[derive(Deserialize)]
struct Fixture {
    examples: Vec<Example>,
    refusals: Vec<Refusal>,
    rejected: Vec<Rejected>,
    long_floats: Vec<LongFloat>,
}

#[derive(Deserialize)]
struct Rejected {
    model: String,
    name: String,
    given: Value,
}

#[derive(Deserialize)]
struct LongFloat {
    text: String,
    bits: u64,
    request: String,
    call: String,
    arguments: String,
}

#[derive(Deserialize)]
struct Refusal {
    model: String,
    name: String,
    given: Value,
    error: String,
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

/// The JSON text `T` writes once it has read `value`. A value and its JSON
/// text are two readers: both must read alike.
fn rewritten<T: DeserializeOwned + Serialize>(value: &Value) -> serde_json::Result<String> {
    let from_value = T::deserialize(value).and_then(|read| serde_json::to_string(&read));
    let from_text =
        serde_json::from_str::<T>(&value.to_string()).and_then(|read| serde_json::to_string(&read));
    assert_eq!(
        from_value.as_ref().ok(),
        from_text.as_ref().ok(),
        "a value and its JSON text read alike: {value}"
    );
    from_value
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
        // The response is the fields a provider fills of Python's response.
        "AIResponse" => rewritten::<AiResponse>(value),
        "AIToolCall" => rewritten::<AiToolCall>(value),
        "ServedCall" => rewritten::<ServedCall>(value),
        "StreamTextDelta" => rewritten::<StreamTextDelta>(value),
        "StreamThinkingDelta" => rewritten::<StreamThinkingDelta>(value),
        "StreamToolCall" => rewritten::<StreamToolCall>(value),
        "StreamToolCallDelta" => rewritten::<StreamToolCallDelta>(value),
        "StreamUsage" => rewritten::<StreamUsage>(value),
        "StreamDone" => rewritten::<StreamDone>(value),
        "ModelInfo" => rewritten::<ModelInfo>(value),
        "ModelPricing" => rewritten::<ModelPricing>(value),
        // Python has no model for these two: the union of its event models,
        // and the day `date.fromisoformat` reads.
        "StreamEvent" => rewritten::<StreamEvent>(value),
        "date" => rewritten::<Date>(value),
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

/// What Python refuses when it builds a model is refused when it is read,
/// in Python's words: a response schema outside the portable subset, a tool
/// name no vendor accepts.
#[test]
fn the_fields_python_refuses_are_refused_in_its_words() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    let refused: Vec<(String, Option<String>)> = fixture
        .refusals
        .iter()
        .map(|refusal| {
            let label = format!("{} `{}`", refusal.model, refusal.name);
            let error = rewritten_as(&refusal.model, &refusal.given).err();
            (label, error.map(|error| error.to_string()))
        })
        .collect();
    let expected: Vec<(String, Option<String>)> = fixture
        .refusals
        .iter()
        .map(|refusal| {
            let label = format!("{} `{}`", refusal.model, refusal.name);
            (label, Some(refusal.error.clone()))
        })
        .collect();
    assert_eq!(refused, expected);
    assert!(refused.len() >= 18, "the fixture lost its refusals");
}

/// What Python refuses in pydantic's own words is refused too: a model given
/// as a list of its values, a name or a `type` given as anything but a
/// text.
#[test]
fn the_inputs_python_rejects_are_rejected() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    let taken: Vec<String> = fixture
        .rejected
        .iter()
        .filter(|input| rewritten_as(&input.model, &input.given).is_ok())
        .map(|input| format!("{} `{}`", input.model, input.name))
        .collect();
    assert_eq!(taken, Vec::<String>::new(), "taken where Python refuses");
    assert!(fixture.rejected.len() >= 53, "the fixture lost inputs");
}

/// A float Python writes with 16 or 17 digits is read to the bit, in a
/// request's temperature, in metadata and in a call's arguments, and is
/// written back as Python wrote it.
#[test]
fn a_long_float_is_read_to_the_bit_and_written_back() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    let bits = |value: &Value| value.as_f64().map(f64::to_bits);
    for float in &fixture.long_floats {
        let text = &float.text;
        let request: AiRequest = serde_json::from_str(&float.request).unwrap();
        assert_eq!(request.temperature.to_bits(), float.bits, "{text}");
        assert_eq!(bits(&request.metadata["ratio"]), Some(float.bits), "{text}");
        assert_eq!(serde_json::to_string(&request).unwrap(), float.request);

        let call: AiToolCall = serde_json::from_str(&float.call).unwrap();
        assert_eq!(bits(&call.arguments["ratio"]), Some(float.bits), "{text}");
        assert_eq!(serde_json::to_string(&call).unwrap(), float.call);

        let arguments = tool_arguments(&float.arguments);
        assert_eq!(bits(&arguments["ratio"]), Some(float.bits), "{text}");
    }
    assert!(
        fixture.long_floats.len() >= 4,
        "the fixture lost its floats"
    );
}

/// The types Python has no model for hold the same JSON: a message's content
/// and each of its parts, a tool's result and each of its parts, a role, an
/// outcome, an event of a stream and a price's day. So do the models a form
/// holds: a response's calls and reasoning blocks, a model's price.
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
        if example.model.starts_with("Stream") {
            check(
                rewritten::<StreamEvent>(&example.form),
                &example.form,
                &label,
            );
        }
        if example.model == "AIResponse" {
            for call in example.form["tool_calls"].as_array().into_iter().flatten() {
                check(rewritten::<AiToolCall>(call), call, &label);
            }
            let blocks = &example.form["thinking_parts"];
            for block in blocks.as_array().into_iter().flatten() {
                check(rewritten::<AiThinkingPart>(block), block, &label);
            }
        }
        if example.model == "ModelInfo" {
            let pricing = &example.form["pricing"];
            check(rewritten::<Option<ModelPricing>>(pricing), pricing, &label);
        }
        if example.model == "ModelPricing" {
            let verified = &example.form["verified"];
            check(rewritten::<Date>(verified), verified, &label);
        }
    }
}

/// The forms the fixture holds of the Python `model`.
fn forms_of<'a>(fixture: &'a Fixture, model: &str) -> Vec<&'a Value> {
    fixture
        .examples
        .iter()
        .filter(|example| example.model == model)
        .map(|example| &example.form)
        .collect()
}

/// The strings among `values`.
fn strings<'a>(values: impl IntoIterator<Item = &'a Value>) -> BTreeSet<String> {
    values
        .into_iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// `names` as a set.
fn named(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|&name| name.to_owned()).collect()
}

/// The members of the lists the `forms` hold under `field`.
fn listed<'a>(forms: &[&'a Value], field: &str) -> Vec<&'a Value> {
    forms
        .iter()
        .filter_map(|form| form[field].as_array())
        .flatten()
        .collect()
}

// The four tests below keep the fixture holding what the tests above are
// for: a regenerated fixture that lost a model or a shape fails here.

#[test]
fn the_fixture_covers_every_model() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
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
            "AIResponse",
            "AITextPart",
            "AIThinkingPart",
            "AITool",
            "AIToolCall",
            "AIToolCallPart",
            "AIToolResultPart",
            "ModelInfo",
            "ModelPricing",
            "ServedCall",
            "StreamDone",
            "StreamTextDelta",
            "StreamThinkingDelta",
            "StreamToolCall",
            "StreamToolCallDelta",
            "StreamUsage",
        ])
    );
}

/// Every role, both shapes of a content and of a result, every part among
/// them, every outcome and none.
#[test]
fn the_fixture_covers_every_shape_of_a_message_and_of_a_result() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    let messages = forms_of(&fixture, "AIMessage");
    assert_eq!(
        strings(messages.iter().map(|message| &message["role"])),
        named(&["assistant", "system", "tool", "user"])
    );
    assert!(
        messages
            .iter()
            .any(|message| message["content"].is_string())
    );
    let parts = listed(&messages, "content");
    assert_eq!(
        strings(parts.iter().map(|part| &part["type"])),
        named(&["image", "text", "thinking", "tool_call", "tool_result"])
    );

    let results = forms_of(&fixture, "AIToolResultPart");
    assert!(results.iter().any(|result| result["result"].is_string()));
    let result_parts = listed(&results, "result");
    assert_eq!(
        strings(result_parts.iter().map(|part| &part["type"])),
        named(&["image", "text"])
    );
    assert_eq!(
        strings(results.iter().map(|result| &result["outcome"])),
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
}

/// A credential, dumped masked; an empty string and a value that is no
/// string, dumped as they are.
#[test]
fn the_fixture_covers_what_python_dumps_under_a_request_s_api_key() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    let requests = forms_of(&fixture, "AIContext");
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

/// Every event of a stream, a response with and without calls and reasoning
/// blocks, a call served or not, a model with and without a price.
#[test]
fn the_fixture_covers_every_event_and_every_shape_of_a_response_and_of_a_model() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    let events = fixture
        .examples
        .iter()
        .filter(|example| example.model.starts_with("Stream"))
        .map(|example| &example.form["type"]);
    assert_eq!(
        strings(events),
        named(&[
            "done",
            "text_delta",
            "thinking_delta",
            "tool_call",
            "tool_call_delta",
            "usage",
        ])
    );

    let responses = forms_of(&fixture, "AIResponse");
    assert_ne!(listed(&responses, "tool_calls").len(), 0);
    assert_ne!(listed(&responses, "thinking_parts").len(), 0);
    assert!(
        responses
            .iter()
            .any(|response| response["thinking_parts"].is_null())
    );
    let calls = forms_of(&fixture, "AIToolCall");
    assert!(calls.iter().any(|call| call["served"].is_null()));
    assert!(calls.iter().any(|call| call["served"].is_object()));

    let models = forms_of(&fixture, "ModelInfo");
    assert!(models.iter().any(|model| model["pricing"].is_null()));
    assert!(models.iter().any(|model| model["pricing"].is_object()));
}
