//! What the contract's types read from JSON, and what they refuse: a model
//! from an object only, a name and a `type` from a string only, and the
//! differences from Python's reading the crate's documentation lists.

use roomkit_providers_core::{
    AiContentPart, AiImagePart, AiMessage, AiMessageContent, AiRequest, AiResponse, AiRole,
    AiTextPart, AiThinkingPart, AiTool, AiToolCall, AiToolCallPart, AiToolResultPart, Date,
    ModelInfo, ModelPricing, ServedCall, StreamDone, StreamEvent, StreamTextDelta,
    StreamThinkingDelta, StreamToolCall, StreamToolCallDelta, StreamUsage, ToolCallOutcome,
    ToolResult, ToolResultPart, tool_arguments,
};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

/// Whether `T` refuses `input`. A value and its JSON text are two readers:
/// both must decide alike.
fn refused<T: DeserializeOwned>(input: &Value) -> bool {
    let from_value = T::deserialize(input).is_err();
    let from_text = serde_json::from_str::<T>(&input.to_string()).is_err();
    assert_eq!(
        from_value, from_text,
        "a value and its text read alike: {input}"
    );
    from_value
}

/// Whether `T` refuses the JSON `text`.
fn refused_text<T: DeserializeOwned>(text: &str) -> bool {
    serde_json::from_str::<T>(text).is_err()
}

/// The names of the types that take `input`, among every type read from an
/// object: each model, and each union of models.
fn object_types_taking(input: &Value) -> Vec<&'static str> {
    let mut taking = Vec::new();
    macro_rules! tried {
        ($($model:ty),+ $(,)?) => {
            $(
                if !refused::<$model>(input) {
                    taking.push(stringify!($model));
                }
            )+
        };
    }
    tried!(
        AiRequest,
        AiMessage,
        AiTextPart,
        AiImagePart,
        AiThinkingPart,
        AiToolCallPart,
        AiToolResultPart,
        AiContentPart,
        AiTool,
        ToolResultPart,
        AiResponse,
        AiToolCall,
        ServedCall,
        StreamEvent,
        StreamTextDelta,
        StreamThinkingDelta,
        StreamToolCall,
        StreamToolCallDelta,
        StreamUsage,
        StreamDone,
        ModelInfo,
        ModelPricing,
    );
    taking
}

#[test]
fn a_model_is_read_from_an_object_and_from_nothing_else() {
    for input in [
        json!([]),
        json!(["user", "hi"]),
        json!([[], "Answer briefly.", 0.1]),
        json!(["done"]),
        json!(null),
        json!("text"),
        json!(3),
        json!(true),
    ] {
        assert_eq!(object_types_taking(&input), [""; 0], "{input}");
    }
    // The same types do read an object.
    assert!(!refused::<AiRequest>(&json!({})));
    assert!(!refused::<ServedCall>(&json!({})));
    assert!(!refused::<StreamEvent>(&json!({ "type": "done" })));
}

#[test]
fn a_name_is_read_from_a_string_and_from_nothing_else() {
    for name in ["system", "user", "assistant", "tool"] {
        assert!(!refused::<AiRole>(&json!(name)), "{name}");
    }
    for name in [
        "served",
        "refused",
        "failed",
        "blocked",
        "unserved",
        "cancelled",
    ] {
        assert!(!refused::<ToolCallOutcome>(&json!(name)), "{name}");
    }
    for input in [
        json!({ "user": null }),
        json!({ "served": null }),
        json!(["user"]),
        json!(0),
        json!(null),
        json!(true),
        json!("developer"),
        json!("User"),
    ] {
        assert!(refused::<AiRole>(&input), "{input}");
        assert!(refused::<ToolCallOutcome>(&input), "{input}");
    }
    assert!(refused::<Date>(&json!(["2026-08-05"])));
    assert!(refused::<Date>(&json!({ "2026-08-05": null })));
}

/// `fields`, with `tag` as its `type`, written first or last.
fn typed(fields: &Value, tag: &Value, first: bool) -> Value {
    let mut object = Map::new();
    if first {
        object.insert("type".to_owned(), tag.clone());
    }
    for (name, value) in fields.as_object().into_iter().flatten() {
        object.insert(name.clone(), value.clone());
    }
    if !first {
        object.insert("type".to_owned(), tag.clone());
    }
    Value::Object(object)
}

/// What stands for the `type` `literal` in a looser reading: a number, an
/// object of one key, a list, nothing, another name.
fn no_type(literal: &str) -> [Value; 5] {
    [
        json!(0),
        json!({ literal: null }),
        json!([literal]),
        json!(null),
        json!("other"),
    ]
}

/// Checks a model read alone: it takes its own `type`, wherever it is
/// written, or none; it refuses anything else for it.
fn alone<T: DeserializeOwned>(literal: &str, fields: &Value) {
    assert!(!refused::<T>(fields), "{literal} without a type");
    for first in [true, false] {
        assert!(
            !refused::<T>(&typed(fields, &json!(literal), first)),
            "{literal}"
        );
        for tag in no_type(literal) {
            assert!(
                refused::<T>(&typed(fields, &tag, first)),
                "{literal}: {tag}"
            );
        }
    }
}

/// Checks a model read as one of a union `U`: its `type` is required, a
/// string, wherever it is written.
fn in_union<U: DeserializeOwned>(literal: &str, fields: &Value) {
    assert!(refused::<U>(fields), "{literal} without a type");
    for first in [true, false] {
        assert!(
            !refused::<U>(&typed(fields, &json!(literal), first)),
            "{literal}"
        );
        for tag in no_type(literal) {
            assert!(
                refused::<U>(&typed(fields, &tag, first)),
                "{literal}: {tag}"
            );
        }
    }
}

#[test]
fn a_type_is_read_from_a_string_and_from_nothing_else() {
    let call = json!({ "id": "call_1", "name": "lookup" });
    let result = json!({ "tool_call_id": "call_1", "name": "lookup", "result": "done" });
    let text = json!({ "text": "hi" });
    let image = json!({ "url": "https://example.com/a.png" });
    let thinking = json!({ "thinking": "Look it up." });

    alone::<AiTextPart>("text", &text);
    alone::<AiImagePart>("image", &image);
    alone::<AiThinkingPart>("thinking", &thinking);
    alone::<AiToolCallPart>("tool_call", &call);
    alone::<AiToolResultPart>("tool_result", &result);
    alone::<StreamTextDelta>("text_delta", &text);
    alone::<StreamThinkingDelta>("thinking_delta", &thinking);
    alone::<StreamToolCall>("tool_call", &call);
    alone::<StreamToolCallDelta>("tool_call_delta", &call);
    alone::<StreamUsage>("usage", &json!({}));
    alone::<StreamDone>("done", &json!({}));

    in_union::<AiContentPart>("text", &text);
    in_union::<AiContentPart>("image", &image);
    in_union::<AiContentPart>("thinking", &thinking);
    in_union::<AiContentPart>("tool_call", &call);
    in_union::<AiContentPart>("tool_result", &result);
    in_union::<ToolResultPart>("text", &text);
    in_union::<ToolResultPart>("image", &image);
    in_union::<StreamEvent>("text_delta", &text);
    in_union::<StreamEvent>("thinking_delta", &thinking);
    in_union::<StreamEvent>("tool_call", &call);
    in_union::<StreamEvent>("tool_call_delta", &call);
    in_union::<StreamEvent>("usage", &json!({}));
    in_union::<StreamEvent>("done", &json!({}));
}

/// A message holding a tool result whose own part has `inner` as its
/// `type`, each `type` written first or last.
fn nested(inner: &Value, first: bool) -> Value {
    let part = typed(&json!({ "text": "the page" }), inner, first);
    let result = typed(
        &json!({ "tool_call_id": "call_1", "name": "open_page", "result": [part] }),
        &json!("tool_result"),
        first,
    );
    json!({ "messages": [{ "role": "tool", "content": [result] }] })
}

/// A part held back until its `type` is found is read as one read as it
/// comes, down to the parts it holds.
#[test]
fn an_object_reads_alike_wherever_its_type_is_written() {
    let read = |first: bool| serde_json::from_value::<AiRequest>(nested(&json!("text"), first));
    let (typed_first, typed_last) = (read(true).unwrap(), read(false).unwrap());
    assert_eq!(typed_first, typed_last);
    assert_eq!(
        typed_last.messages[0]
            .tool_result()
            .map(|part| &part.result),
        Some(&ToolResult::Parts(vec![ToolResultPart::Text(
            AiTextPart::new("the page")
        )]))
    );
    for first in [true, false] {
        for tag in no_type("text") {
            assert!(refused::<AiRequest>(&nested(&tag, first)), "{tag} {first}");
        }
    }
}

/// The documentation's list of what Python takes and the contract refuses.
#[test]
fn input_is_stricter_than_python_s_where_the_documentation_says() {
    let pricing = |verified: Value| json!({ "input_per_million": 1, "output_per_million": 1, "verified": verified });
    // A role Python has no list of.
    assert!(refused::<AiMessage>(
        &json!({ "role": "developer", "content": "x" })
    ));
    // A part without its type in a list, an event without its type.
    assert!(refused::<AiMessageContent>(&json!([{ "text": "x" }])));
    assert!(refused::<ToolResult>(&json!([{ "text": "x" }])));
    assert!(refused::<StreamEvent>(&json!({ "text": "x" })));
    // A value pydantic converts.
    assert!(refused::<AiRequest>(&json!({ "temperature": "0.7" })));
    assert!(refused::<AiRequest>(&json!({ "max_tokens": 1.0 })));
    assert!(refused::<AiRequest>(&json!({ "enable_thinking": 1 })));
    assert!(refused::<AiResponse>(
        &json!({ "content": "", "usage": { "tokens": "12" } })
    ));
    assert!(refused::<ModelPricing>(&pricing(json!(1_785_888_000))));
    assert!(refused::<ModelPricing>(&pricing(json!(
        "2026-08-05T00:00:00"
    ))));
    assert!(!refused::<ModelPricing>(&pricing(json!("2026-08-05"))));
    // An integer outside its field's type.
    assert!(refused::<AiRequest>(&json!({ "max_tokens": -5 })));
    assert!(refused::<AiRequest>(
        &json!({ "max_tokens": 4_294_967_296_u64 })
    ));
    assert!(!refused::<AiRequest>(
        &json!({ "max_tokens": 4_294_967_295_u32 })
    ));
    let delta = json!({ "id": "call_1", "name": "lookup", "index": -1 });
    assert!(refused::<StreamToolCallDelta>(&delta));
    assert!(refused::<StreamThinkingDelta>(
        &json!({ "thinking": "", "block": -1 })
    ));
    assert!(refused::<ModelInfo>(
        &json!({ "id": "m", "context_window": -1 })
    ));
}

/// The documentation's list of what the JSON parser reads differently from
/// Python's.
#[test]
fn the_parser_reads_differently_from_python_s_where_the_documentation_says() {
    // An integer past 64 bits is a float, which no integer field takes.
    let past = tool_arguments(r#"{"n": 18446744073709551616}"#);
    assert_eq!(past["n"].as_f64(), Some(18_446_744_073_709_551_616.0));
    assert!(past["n"].is_f64());
    assert!(refused_text::<AiRequest>(
        r#"{"thinking_budget": 18446744073709551616}"#
    ));
    assert!(refused_text::<AiResponse>(
        r#"{"content": "", "usage": {"tokens": 18446744073709551616}}"#
    ));
    // Nesting of 128 levels or more.
    let nested = |levels: usize| {
        let lists = levels - 1;
        format!("{{\"a\": {}1{}}}", "[".repeat(lists), "]".repeat(lists))
    };
    assert!(!tool_arguments(&nested(127)).contains_key("raw"));
    assert!(tool_arguments(&nested(128)).contains_key("raw"));
    let call = |arguments: &str| format!(r#"{{"id": "c", "name": "n", "arguments": {arguments}}}"#);
    assert!(!refused_text::<AiToolCall>(&call(&nested(126))));
    assert!(refused_text::<AiToolCall>(&call(&nested(127))));
    // A lone surrogate escape.
    let lone = "{\"a\": \"\\ud800\"}";
    assert!(tool_arguments(lone).contains_key("raw"));
    assert!(refused_text::<AiToolCall>(&call(lone)));
    // A negative zero.
    let zero = tool_arguments(r#"{"z": -0}"#);
    assert_eq!(serde_json::to_string(&zero).unwrap(), r#"{"z":-0.0}"#);
    // A number JSON does not name, or past a float's range.
    for number in ["NaN", "Infinity", "-Infinity", "1e999"] {
        let request = format!(r#"{{"temperature": {number}}}"#);
        assert!(refused_text::<AiRequest>(&request), "{number}");
    }
    // A field written twice in a model's object.
    assert!(refused_text::<AiRequest>(
        r#"{"temperature": 0.1, "temperature": 0.2}"#
    ));
    assert_eq!(tool_arguments(r#"{"a": 1, "a": 2}"#)["a"], 2);
}

/// An unknown member of an object is left out, as Python leaves it out.
#[test]
fn a_member_a_model_does_not_have_is_left_out() {
    let tool: AiTool = serde_json::from_value(json!({
        "name": "lookup",
        "description": "Find a value",
        "version": 3,
    }))
    .unwrap();
    assert_eq!(tool, AiTool::new("lookup", "Find a value").unwrap());
    let event: StreamEvent =
        serde_json::from_str(r#"{"note": [1, {"deep": null}], "type": "done", "extra": 1}"#)
            .unwrap();
    assert_eq!(event, StreamEvent::Done(StreamDone::default()));
}

/// A union's object is held until its `type` is found when the `type` does
/// not come first. What a member written twice reads as must not depend on
/// where the `type` is written.
#[test]
fn a_member_written_twice_reads_alike_wherever_the_type_is_written() {
    let read = |text: &str| serde_json::from_str::<AiContentPart>(text).ok();
    let text = |said: &str| Some(AiContentPart::Text(AiTextPart::new(said)));
    // A field of the model written twice is refused.
    assert_eq!(read(r#"{"type":"text","text":"a","text":"b"}"#), None);
    assert_eq!(read(r#"{"text":"a","text":"b","type":"text"}"#), None);
    assert_eq!(read(r#"{"text":"a","type":"text","text":"b"}"#), None);
    // A member the model does not have is left out, written once or twice.
    assert_eq!(
        read(r#"{"type":"text","text":"a","note":1,"note":2}"#),
        text("a")
    );
    assert_eq!(
        read(r#"{"note":1,"note":2,"text":"a","type":"text"}"#),
        text("a")
    );
    // The first `type` says which model the object is; another one is the
    // model's own, which names that model or is refused.
    assert_eq!(
        read(r#"{"type":"text","type":"text","text":"a"}"#),
        text("a")
    );
    assert_eq!(
        read(r#"{"text":"a","type":"text","type":"text"}"#),
        text("a")
    );
    assert_eq!(read(r#"{"type":"text","type":"image","text":"a"}"#), None);
    assert_eq!(read(r#"{"text":"a","type":"text","type":"image"}"#), None);
    assert_eq!(
        read(r#"{"type":"text","type":"text","type":"text","text":"a"}"#),
        None
    );
    assert_eq!(
        read(r#"{"text":"a","type":"text","type":"text","type":"text"}"#),
        None
    );
    // The same holds for an event of a stream.
    let event = |text: &str| serde_json::from_str::<StreamEvent>(text).ok();
    assert_eq!(
        event(r#"{"type":"text_delta","text":"a","text":"b"}"#),
        None
    );
    assert_eq!(
        event(r#"{"text":"a","text":"b","type":"text_delta"}"#),
        None
    );
}
