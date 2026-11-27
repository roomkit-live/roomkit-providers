//! Cerebras's arguments read back (Python's `_decode_arrays` of
//! `roomkit.providers.cerebras`): an array the model wrote as the JSON text
//! of the array, read as the array, where the tool's schema declares one.
//! A transport's repair, not a coercion: a scalar, a text that does not read
//! or reads as no array, a schema that does not say, and a member the schema
//! does not name stay as they came, for the guards after.

use roomkit::model::Metadata;
use roomkit_ai::{AiResponse, AiTool, ProviderError, ProviderStream, StreamEvent};
use roomkit_providers_client::items::{Yielder, items, next};
use serde_json::Value;

/// `response`, each call's arguments read back by its tool's schema among
/// `tools` (Python's `CerebrasAIProvider.generate`).
pub(crate) fn repaired_response(mut response: AiResponse, tools: &[AiTool]) -> AiResponse {
    for call in &mut response.tool_calls {
        let arguments = std::mem::take(&mut call.arguments);
        call.arguments = repaired_arguments(arguments, &call.name, tools);
    }
    response
}

/// `stream`, each whole call's arguments read back by its tool's schema
/// among `tools`; its fragments stay as they came (Python's
/// `CerebrasAIProvider.generate_structured_stream`). Dropping it drops
/// `stream`, which stops its request.
pub(crate) fn repaired_stream<'a>(
    stream: ProviderStream<'a>,
    tools: &'a [AiTool],
) -> ProviderStream<'a> {
    type Item = Result<StreamEvent, ProviderError>;
    Box::pin(items(move |out: Yielder<Item>| async move {
        let mut stream = stream;
        while let Some(item) = next(&mut stream).await {
            let item = item.map(|event| {
                let StreamEvent::ToolCall(mut call) = event else {
                    return event;
                };
                let arguments = std::mem::take(&mut call.arguments);
                call.arguments = repaired_arguments(arguments, &call.name, tools);
                StreamEvent::ToolCall(call)
            });
            out.give(item).await;
        }
    }))
}

/// `arguments` of a call to `name`, its arrays written as text read back by
/// the schema of the tool of that name among `tools`; as they are for a
/// tool `tools` does not hold.
pub(crate) fn repaired_arguments(arguments: Metadata, name: &str, tools: &[AiTool]) -> Metadata {
    let Some(tool) = tools.iter().find(|tool| tool.name == name) else {
        return arguments;
    };
    let schema = &tool.parameters;
    let kind = schema.get("type");
    let object_kind = kind.is_none() || kind == Some(&Value::from("object"));
    let Some(Value::Object(properties)) = schema.get("properties").filter(|_| object_kind) else {
        return arguments;
    };
    arguments
        .into_iter()
        .map(|(member, value)| {
            let read = match properties.get(&member) {
                Some(property @ Value::Object(_)) => decoded(value, property),
                _ => value,
            };
            (member, read)
        })
        .collect()
}

/// `value` read by `schema`: a text where an array is declared read as the
/// array it writes, and the items of an array and the members of an object
/// read by the schema of each.
fn decoded(value: Value, schema: &Value) -> Value {
    let kind = schema.get("type");
    let declares_array = kind == Some(&Value::from("array"));
    let value = match (declares_array, value) {
        (true, Value::String(text)) => match serde_json::from_str::<Value>(&text) {
            Ok(array @ Value::Array(_)) => array,
            _ => Value::String(text),
        },
        (_, value) => value,
    };
    if declares_array {
        return match (value, schema.get("items")) {
            (Value::Array(items), Some(item @ Value::Object(_))) => {
                Value::Array(items.into_iter().map(|each| decoded(each, item)).collect())
            }
            (value, _) => value,
        };
    }
    let object_kind = kind.is_none() || kind == Some(&Value::from("object"));
    match (object_kind, value) {
        (true, Value::Object(members)) => {
            let Some(Value::Object(properties)) = schema.get("properties") else {
                return Value::Object(members);
            };
            let read = members.into_iter().map(|(name, member)| {
                let read = match properties.get(&name) {
                    Some(property @ Value::Object(_)) => decoded(member, property),
                    _ => member,
                };
                (name, read)
            });
            Value::Object(read.collect())
        }
        (_, value) => value,
    }
}
