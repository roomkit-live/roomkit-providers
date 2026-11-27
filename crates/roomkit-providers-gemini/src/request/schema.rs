//! A tool's parameters as Gemini declares them (Python's
//! `clean_gemini_schema` and `function_declaration`, then the google-genai
//! SDK's `Schema` as it writes it): a union and a type list folded into the
//! one type Gemini takes, the shape JSON Schema leaves implied spelled out,
//! an enum Gemini cannot hold described, every other key dropped, and
//! `required` kept for the properties that survived. The SDK then writes a
//! type it knows in capitals, a bound as a number with a fraction, and
//! `minItems`/`maxItems` as `min_items`/`max_items`.

use serde_json::{Map, Value};

/// The keys Gemini takes in a function parameter schema.
const ALLOWED: [&str; 12] = [
    "type",
    "properties",
    "required",
    "description",
    "enum",
    "items",
    "format",
    "nullable",
    "minimum",
    "maximum",
    "minItems",
    "maxItems",
];

/// The keys of a union.
const UNION_KEYS: [&str; 3] = ["anyOf", "oneOf", "allOf"];

/// The types the SDK knows, written in capitals.
const TYPES: [&str; 7] = [
    "string", "number", "integer", "boolean", "array", "object", "null",
];

type Object = Map<String, Value>;

/// `parameters`, the schema a tool is declared with, as the request carries
/// it under `parameters`.
pub(crate) fn declared(parameters: &Value) -> Value {
    let cleaned = parameters
        .as_object()
        .map_or_else(Object::new, |object| clean(object.clone()));
    written(Value::Object(cleaned))
}

/// Python's `_clean`: the passes of one node, then its kept keys, its
/// properties and its items cleaned in turn.
fn clean(object: Object) -> Object {
    let object = collapse_type_list(object);
    let object = collapse_union(object);
    let object = collapse_type_list(object);
    let object = complete_shape(object);
    let object = describe_enum(object);
    let mut result = Object::new();
    for (key, value) in object {
        if !ALLOWED.contains(&key.as_str()) {
            continue;
        }
        let value = match (key.as_str(), value) {
            ("properties", Value::Object(properties)) => {
                let kept = properties.into_iter().filter_map(|(name, schema)| {
                    let Value::Object(schema) = schema else {
                        return None;
                    };
                    Some((name, Value::Object(clean(schema))))
                });
                Value::Object(kept.collect())
            }
            ("items", Value::Object(items)) => Value::Object(clean(items)),
            (_, value) => value,
        };
        result.insert(key, value);
    }
    keep_declared_required(result)
}

/// Python's `_collapse_type_list`: a type list as its first non-null type,
/// `string` where it names none, `nullable` where it named `null`.
fn collapse_type_list(mut object: Object) -> Object {
    let Some(Value::Array(types)) = object.get("type") else {
        return object;
    };
    let named: Vec<&str> = types.iter().filter_map(Value::as_str).collect();
    let non_null: Vec<&str> = named
        .iter()
        .copied()
        .filter(|kind| *kind != "null")
        .collect();
    let nullable = named.len() != non_null.len();
    let kind = non_null.first().copied().unwrap_or("string").to_owned();
    object.insert("type".to_owned(), Value::String(kind));
    if nullable {
        object.insert("nullable".to_owned(), Value::Bool(true));
    }
    object
}

/// The branches of `object`'s first union key holding a list, and that key.
fn union_of(object: &Object) -> Option<(&'static str, Vec<Object>)> {
    let key = UNION_KEYS
        .into_iter()
        .find(|key| matches!(object.get(*key), Some(Value::Array(_))))?;
    let Some(Value::Array(branches)) = object.get(key) else {
        return None;
    };
    let branches = branches
        .iter()
        .filter_map(|branch| branch.as_object().cloned())
        .collect();
    Some((key, branches))
}

fn is_null_branch(branch: &Object) -> bool {
    branch.get("type").and_then(Value::as_str) == Some("null")
}

/// Python's `_collapse_union`: a union folded into one branch, but one that
/// narrows its node, which keeps the node.
fn collapse_union(object: Object) -> Object {
    let Some((key, branches)) = union_of(&object) else {
        return object;
    };
    if refines_node(&object, &branches) {
        return keep_node(object, &branches);
    }
    let has_null = branches.iter().any(is_null_branch);
    let non_null: Vec<Object> = branches
        .iter()
        .filter(|branch| !is_null_branch(branch))
        .cloned()
        .collect();
    let mut merged = match non_null.first() {
        None => {
            let mut fallback = Object::new();
            fallback.insert("type".to_owned(), Value::String("string".to_owned()));
            fallback.insert("nullable".to_owned(), Value::Bool(true));
            fallback
        }
        Some(first) if key == "allOf" => intersect(&non_null, first.clone()),
        Some(first) => first.clone(),
    };
    if has_null && !non_null.is_empty() {
        merged.insert("nullable".to_owned(), Value::Bool(true));
    }
    for kept in ["description", "title", "nullable"] {
        let value = object.get(kept).filter(|_| !merged.contains_key(kept));
        if let Some(value) = value {
            merged.insert(kept.to_owned(), value.clone());
        }
    }
    collapse_union(merged)
}

/// Python's `_refines_node`: a node with a structure of its own, or a union
/// whose alternatives name no shape, is narrowed, not offered a choice.
fn refines_node(object: &Object, branches: &[Object]) -> bool {
    if object.contains_key("properties") || object.contains_key("items") {
        return true;
    }
    let alternatives: Vec<&Object> = branches.iter().filter(|b| !is_null_branch(b)).collect();
    !alternatives.is_empty() && !alternatives.iter().any(|branch| names_a_shape(branch))
}

fn names_a_shape(branch: &Object) -> bool {
    branch.contains_key("type")
        || UNION_KEYS
            .iter()
            .any(|key| matches!(branch.get(*key), Some(Value::Array(_))))
}

/// Python's `_keep_node`: the node, nullable under a null branch, an
/// object gaining the properties its branches add.
fn keep_node(object: Object, branches: &[Object]) -> Object {
    let mut kept = object;
    if branches.iter().any(is_null_branch) {
        kept.insert("nullable".to_owned(), Value::Bool(true));
    }
    let kind = kept.get("type").and_then(Value::as_str).unwrap_or("object");
    let own = match kept.get("properties") {
        None => Some(Object::new()),
        Some(Value::Object(own)) => Some(own.clone()),
        Some(_) => None,
    };
    let (true, Some(own)) = (kind == "object" && !kept.contains_key("items"), own) else {
        return kept;
    };
    let properties = gather_properties(own.clone(), branches);
    if properties.len() == own.len() {
        return kept;
    }
    kept.insert("properties".to_owned(), Value::Object(properties));
    kept
}

/// Python's `_intersect`: the first branch of an `allOf` with every other
/// branch's properties and required names.
fn intersect(branches: &[Object], first: Object) -> Object {
    let mut merged = first;
    let own = match merged.get("properties") {
        Some(Value::Object(own)) => own.clone(),
        _ => Object::new(),
    };
    let properties = gather_properties(own, branches);
    let mut required: Vec<Value> = Vec::new();
    for branch in branches {
        if let Some(Value::Array(names)) = branch.get("required") {
            for name in names {
                if !required.contains(name) {
                    required.push(name.clone());
                }
            }
        }
    }
    if !properties.is_empty() {
        merged.insert("properties".to_owned(), Value::Object(properties));
    }
    if !required.is_empty() {
        merged.insert("required".to_owned(), Value::Array(required));
    }
    merged
}

/// `own` with the properties `branches` add, a name `own` has winning.
fn gather_properties(own: Object, branches: &[Object]) -> Object {
    let mut properties = own;
    for branch in branches {
        if let Some(Value::Object(added)) = branch.get("properties") {
            for (name, schema) in added {
                properties
                    .entry(name.clone())
                    .or_insert_with(|| schema.clone());
            }
        }
    }
    properties
}

/// Python's `_complete_shape`: the type an untyped node's keys imply, and no
/// key the type cannot carry; an array's items, an empty schema at least.
fn complete_shape(object: Object) -> Object {
    let kind = match object.get("type") {
        Some(kind) => Some(kind.clone()),
        None if object.contains_key("properties") => Some(Value::String("object".to_owned())),
        None if object.contains_key("items") => Some(Value::String("array".to_owned())),
        None => None,
    };
    let Some(kind) = kind else {
        return object;
    };
    let named = kind.as_str();
    let mut shaped: Object = object
        .into_iter()
        .filter(|(key, _)| {
            (key != "items" || named == Some("array"))
                && (key != "properties" || named == Some("object"))
        })
        .collect();
    shaped.insert("type".to_owned(), kind.clone());
    if named == Some("array") && !matches!(shaped.get("items"), Some(Value::Object(_))) {
        shaped.insert("items".to_owned(), Value::Object(Object::new()));
    }
    shaped
}

/// Python's `_describe_enum`: an enum of strings kept, `null` among it
/// read as `nullable`; any other moved into the description, the node
/// typed as its values are.
fn describe_enum(object: Object) -> Object {
    let Some(Value::Array(values)) = object.get("enum") else {
        return object;
    };
    let values = values.clone();
    let present: Vec<&Value> = values.iter().filter(|value| !value.is_null()).collect();
    let nullable = present.len() != values.len();
    if !present.is_empty() && present.iter().all(|value| value.is_string()) {
        if !nullable {
            return object;
        }
        let mut kept = object;
        let strings = present.into_iter().cloned().collect();
        kept.insert("enum".to_owned(), Value::Array(strings));
        kept.insert("nullable".to_owned(), Value::Bool(true));
        return kept;
    }
    let mut described: Object = object
        .into_iter()
        .filter(|(key, _)| key != "enum")
        .collect();
    if nullable {
        described.insert("nullable".to_owned(), Value::Bool(true));
    }
    let kind = shared_json_type(&present).filter(|_| !described.contains_key("type"));
    if let Some(kind) = kind {
        described.insert("type".to_owned(), Value::String(kind.to_owned()));
    }
    let allowed: Vec<String> = values.iter().map(python_json).collect();
    let note = format!("Allowed values: {}.", allowed.join(", "));
    let description = match described.get("description").and_then(Value::as_str) {
        Some(description) if !description.is_empty() => format!("{description} {note}"),
        _ => note,
    };
    described.insert("description".to_owned(), Value::String(description));
    described
}

/// The JSON type all `values` share, integers widening to numbers; none for
/// a text or a structure, which Python's `_json_type` names no type.
fn shared_json_type(values: &[&Value]) -> Option<&'static str> {
    let kinds: Vec<Option<&'static str>> = values.iter().map(|value| json_type(value)).collect();
    let mut distinct: Vec<Option<&'static str>> = Vec::new();
    for kind in kinds {
        if !distinct.contains(&kind) {
            distinct.push(kind);
        }
    }
    let integers_and_numbers = distinct.len() == 2
        && distinct.contains(&Some("integer"))
        && distinct.contains(&Some("number"));
    if integers_and_numbers {
        return Some("number");
    }
    match distinct.as_slice() {
        [kind] => *kind,
        _ => None,
    }
}

fn json_type(value: &Value) -> Option<&'static str> {
    match value {
        Value::Bool(_) => Some("boolean"),
        Value::Number(number) if number.is_f64() => Some("number"),
        Value::Number(_) => Some("integer"),
        Value::Null | Value::String(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

/// `value` as Python's `json.dumps(value, ensure_ascii=False)` writes a
/// scalar of an enum.
fn python_json(value: &Value) -> String {
    match value {
        Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null => value.to_string(),
        Value::Array(_) | Value::Object(_) => crate::request::json_line(value),
    }
}

/// Python's `_keep_declared_required`: `required` naming only the
/// properties the node keeps, gone when it names none.
fn keep_declared_required(mut result: Object) -> Object {
    let Some(Value::Array(required)) = result.get("required") else {
        return result;
    };
    let declared = match result.get("properties") {
        Some(Value::Object(declared)) => declared.clone(),
        _ => Object::new(),
    };
    let kept: Vec<Value> = required
        .iter()
        .filter(|name| {
            name.as_str()
                .is_some_and(|name| declared.contains_key(name))
        })
        .cloned()
        .collect();
    if kept.is_empty() {
        result.remove("required");
    } else {
        result.insert("required".to_owned(), Value::Array(kept));
    }
    result
}

/// A cleaned schema as the SDK writes it: a type it knows in capitals, a
/// bound with a fraction, the item counts under their snake case names,
/// each nested schema written the same way.
fn written(schema: Value) -> Value {
    let Value::Object(object) = schema else {
        return schema;
    };
    let mut written: Vec<(String, Value)> = Vec::new();
    for (key, value) in object {
        let value = match key.as_str() {
            "type" => match value.as_str() {
                Some(kind) if TYPES.contains(&kind) => Value::String(kind.to_ascii_uppercase()),
                _ => value,
            },
            "minimum" | "maximum" => match value.as_f64() {
                Some(bound) => serde_json::Number::from_f64(bound).map_or(value, Value::Number),
                None => value,
            },
            "properties" => match value {
                Value::Object(properties) => Value::Object(
                    properties
                        .into_iter()
                        .map(|(name, schema)| (name, self::written(schema)))
                        .collect(),
                ),
                other @ (Value::Null
                | Value::Bool(_)
                | Value::Number(_)
                | Value::String(_)
                | Value::Array(_)) => other,
            },
            "items" => self::written(value),
            _ => value,
        };
        let key = match key.as_str() {
            "minItems" => "min_items".to_owned(),
            "maxItems" => "max_items".to_owned(),
            _ => key,
        };
        written.push((key, value));
    }
    // The SDK writes a schema's members in the order of its model's fields,
    // which is the alphabet's; a property's names keep theirs.
    written.sort_by(|(a, _), (b, _)| a.cmp(b));
    Value::Object(written.into_iter().collect())
}
