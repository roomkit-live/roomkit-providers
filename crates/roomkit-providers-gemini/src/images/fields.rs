//! An interaction's steps and usage written back as Python's provider keeps
//! them: as google-genai dumps its models (`model_dump(mode="json",
//! exclude_none=True)`), then cleaned as `plain_metadata` cleans them.
//!
//! A model the SDK knows writes its fields in the order it declares them,
//! then the members it does not declare, as they came; an empty member is
//! left out of either. A step or a content of a kind it does not know is
//! written back as it came. The SDK's models are read from
//! `catalog/interaction_fields.json`, written from the SDK by
//! `scripts/generate_gemini_interaction_fields.py`.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;
use serde_json::{Map, Value};

/// What a member of a model holds.
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Holds {
    /// A model, by its name.
    Model(String),
    /// A list, of what each item holds (none for an item kept as it came).
    List(Option<Box<Holds>>),
    /// A union of models told apart by a member, by its name.
    Union(String),
}

/// A union of models: the member its models are told apart by, and the
/// model of each of its values.
#[derive(Deserialize)]
struct Union {
    key: String,
    variants: HashMap<String, String>,
}

/// The SDK's models, as the file writes them.
#[derive(Deserialize)]
struct Fields {
    steps: Option<Holds>,
    usage: Option<Holds>,
    unions: HashMap<String, Union>,
    /// Each model's fields, in their order: its name, the key it is read
    /// from, and what it holds (none for a value kept as it came).
    models: HashMap<String, Vec<(String, String, Option<Holds>)>>,
}

static FIELDS: LazyLock<Option<Fields>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../catalog/interaction_fields.json")).ok()
});

/// The members `plain_metadata` withholds: payloads and credentials.
const WITHHELD: [&str; 4] = ["data", "b64_json", "api_key", "authorization"];

/// An interaction's `steps`, as Python's provider keeps them.
pub(crate) fn steps(steps: &[Value]) -> Value {
    let steps = Value::Array(steps.to_vec());
    dumped(
        &steps,
        FIELDS.as_ref().and_then(|fields| fields.steps.as_ref()),
    )
}

/// An interaction's `usage`, as Python's provider keeps it.
pub(crate) fn usage(usage: &Value) -> Value {
    dumped(
        usage,
        FIELDS.as_ref().and_then(|fields| fields.usage.as_ref()),
    )
}

/// `value`, of what `holds` says it holds, as the SDK dumps it.
fn dumped(value: &Value, holds: Option<&Holds>) -> Value {
    let fields = FIELDS.as_ref();
    match (holds, fields, value) {
        (Some(Holds::Model(name)), Some(fields), Value::Object(members)) => {
            match fields.models.get(name) {
                Some(model) => model_dumped(members, model),
                None => plain(value),
            }
        }
        (Some(Holds::List(item)), _, Value::Array(items)) => Value::Array(
            items
                .iter()
                .map(|value| dumped(value, item.as_deref()))
                .collect(),
        ),
        (Some(Holds::Union(name)), Some(fields), Value::Object(members)) => {
            let union = fields.unions.get(name);
            let variant = union.and_then(|union| {
                let told = members.get(&union.key).and_then(Value::as_str)?;
                union.variants.get(told)
            });
            match variant {
                Some(model) => dumped(value, Some(&Holds::Model(model.clone()))),
                None => plain(value),
            }
        }
        _ => plain(value),
    }
}

/// The members of a model the SDK knows: its fields in their order, then
/// the members it does not declare, as they came, the empty ones left out.
fn model_dumped(members: &Map<String, Value>, model: &[(String, String, Option<Holds>)]) -> Value {
    let mut written = Map::new();
    for (name, key, holds) in model {
        let Some(value) = members.get(key).filter(|value| !value.is_null()) else {
            continue;
        };
        if !is_withheld(name) {
            written.insert(name.clone(), dumped(value, holds.as_ref()));
        }
    }
    for (key, value) in members {
        let declared = model.iter().any(|(_, declared, _)| declared == key);
        if !declared && !value.is_null() && !is_withheld(key) {
            written.insert(key.clone(), plain(value));
        }
    }
    Value::Object(written)
}

fn is_withheld(name: &str) -> bool {
    WITHHELD.contains(&name) || name.starts_with('_')
}

/// A member the SDK kept as it came, as Python's provider keeps it.
pub(crate) fn raw(value: &Value) -> Value {
    plain(value)
}

/// `value` as it came, its withheld members left out at every depth.
fn plain(value: &Value) -> Value {
    match value {
        Value::Object(members) => {
            let kept = members.iter().filter(|(name, _)| !is_withheld(name));
            Value::Object(
                kept.map(|(name, member)| (name.clone(), plain(member)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(plain).collect()),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => {
            other.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // The file reads whole: a table that no longer reads would write every
    // step as it came.
    #[test]
    fn the_sdk_s_models_read() {
        let read = serde_json::from_str(include_str!("../../catalog/interaction_fields.json"));
        let fields: Fields = read.expect("catalog/interaction_fields.json reads");
        assert!(fields.unions.contains_key("Step"));
        assert!(fields.models.contains_key("Usage"));
    }

    // As google-genai 3.2.0 dumps them, probed: a known step's fields in
    // their order, its empty extra dropped, its extra kept as it came; an
    // unknown step as it came, empty members and all; a payload withheld.
    #[test]
    fn steps_are_written_as_the_sdk_dumps_them() {
        let read = json!([
            {"type": "thought", "content": [{"type": "text", "text": "t", "n": null}], "extra_null": null, "signature": null},
            {"type": "mystery", "b": 1, "a": null, "z": {"y": 1, "x": null}},
            {"type": "model_output", "zz": 1, "content": [
                {"type": "text", "text": "hi", "ex": null},
                {"type": "image", "mime_type": "image/gif", "data": "QUJD"},
                {"type": "weird", "k": null},
            ]},
        ]);
        let written = json!([
            {"type": "thought", "content": [{"type": "text", "text": "t", "n": null}]},
            {"type": "mystery", "b": 1, "a": null, "z": {"y": 1, "x": null}},
            {"content": [{"text": "hi", "type": "text"}, {"mime_type": "image/gif", "type": "image"}, {"type": "weird", "k": null}], "type": "model_output", "zz": 1},
        ]);
        let steps = steps(read.as_array().unwrap());
        assert_eq!(steps.to_string(), written.to_string());
        let read = json!({"total_output_tokens": 3, "zextra": null, "aextra": 2, "output_tokens_by_modality": [{"tokens": 1, "modality": "text", "q": null}]});
        let written = json!({"output_tokens_by_modality": [{"modality": "text", "tokens": 1}], "total_output_tokens": 3, "aextra": 2});
        assert_eq!(usage(&read).to_string(), written.to_string());
    }
}
