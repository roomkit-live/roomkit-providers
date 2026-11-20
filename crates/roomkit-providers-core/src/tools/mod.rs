//! The tools of a request (RFC §6.4, §6.7): what the model is told of a
//! tool, and what a call answered.
//!
//! - [`AiTool`]: a tool as the model is told of it, and
//!   [`declared_parameters`], the schema a provider declares it with.
//! - [`ToolResult`] and [`ToolResultPart`]: what a call answered, a text or
//!   content parts.
//! - [`ToolCallOutcome`]: how a call ended.

mod result;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

pub use self::result::{ToolResult, ToolResultPart};
use crate::error::ValueError;
use crate::strict::{read_as_object, read_name};

/// A tool as the model is told of it (RFC §6.7): Python's `AITool(*, name,
/// description, parameters={}, tags=[], defer_loading=False,
/// changes_state=False)`.
///
/// Read from JSON, a name no vendor accepts is refused, as Python refuses
/// it when the model is built.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AiTool {
    /// The tool's name: one tool per name among a request's tools (RFC
    /// §21.1).
    pub name: String,
    /// What it does, for the model.
    pub description: String,
    /// Its parameters: a JSON Schema object, empty for a tool that takes
    /// none.
    pub parameters: Map<String, Value>,
    /// Keywords a tool search reads beside the name and the description
    /// (RFC §6.4).
    pub tags: Vec<String>,
    /// Declared but unseen: the provider holds the definition out of the
    /// model's view until a tool result references it
    /// ([`AiToolResultPart::references`](crate::AiToolResultPart::references),
    /// RFC §6.4). False by default.
    pub defer_loading: bool,
    /// A call it answers may change what later calls read (a file edited,
    /// a row written), so a host that counts repeated calls starts again
    /// from it (RFC §6.4). False by default.
    pub changes_state: bool,
}

read_as_object!(ToolForm => AiTool);

/// [`AiTool`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a tool, as a JSON object")]
struct ToolForm {
    #[serde(deserialize_with = "accepted_name")]
    name: String,
    description: String,
    #[serde(default)]
    parameters: Map<String, Value>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    defer_loading: bool,
    #[serde(default)]
    changes_state: bool,
}

impl From<ToolForm> for AiTool {
    fn from(form: ToolForm) -> Self {
        Self {
            name: form.name,
            description: form.description,
            parameters: form.parameters,
            tags: form.tags,
            defer_loading: form.defer_loading,
            changes_state: form.changes_state,
        }
    }
}

impl AiTool {
    /// The tool `name`, doing `description`, taking no parameters.
    ///
    /// # Errors
    ///
    /// Returns [`ValueError::ToolName`] for a name no vendor accepts (RFC
    /// §6.7): an empty one, or one with a character other than a letter, a
    /// digit, `_`, `.`, `:` or `-`. A name some vendors refuse is refused by
    /// their provider when it declares the tool.
    ///
    /// ```
    /// use roomkit_providers_core::AiTool;
    /// use serde_json::json;
    ///
    /// let mut lookup = AiTool::new("lookup_order", "Find an order by its number")?;
    /// lookup.parameters.insert("type".into(), json!("object"));
    /// lookup
    ///     .parameters
    ///     .insert("properties".into(), json!({ "order": { "type": "string" } }));
    /// assert!(AiTool::new("files read", "Reads files").is_err());
    /// # Ok::<(), roomkit_providers_core::ValueError>(())
    /// ```
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Result<Self, ValueError> {
        let name = name.into();
        checked(&name)?;
        Ok(Self {
            name,
            description: description.into(),
            parameters: Map::new(),
            tags: Vec::new(),
            defer_loading: false,
            changes_state: false,
        })
    }
}

/// Whether at least one vendor accepts `name` for a tool (RFC §6.7): one or
/// more ASCII letters, digits, `_`, `.`, `:` or `-`. Each provider checks its
/// own vendor's narrower rule before a request.
pub fn some_vendor_accepts_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b':' | b'-'))
}

/// The schema a tool is declared with (RFC §6.7): Python's
/// `declared_parameters(parameters)`. A tool's own schema, its root an
/// object; for a tool that takes no parameters, `None` or an empty schema,
/// an object with none, which every vendor accepts. A root without a type,
/// or whose type is `null`, is said an object's. The schema returned is the
/// caller's own to edit.
///
/// ```
/// use roomkit_providers_core::{AiTool, declared_parameters};
/// use serde_json::json;
///
/// let tool = AiTool::new("now", "The time it is")?;
/// assert_eq!(
///     json!(declared_parameters(Some(&tool.parameters))),
///     json!({ "type": "object", "properties": {} })
/// );
/// # Ok::<(), roomkit_providers_core::ValueError>(())
/// ```
#[must_use]
pub fn declared_parameters(parameters: Option<&Map<String, Value>>) -> Map<String, Value> {
    let Some(parameters) = parameters.filter(|schema| !schema.is_empty()) else {
        let mut none = Map::new();
        none.insert("type".to_owned(), Value::from("object"));
        none.insert("properties".to_owned(), Value::Object(Map::new()));
        return none;
    };
    let mut schema = parameters.clone();
    if schema.get("type").is_none_or(Value::is_null) {
        schema.insert("type".to_owned(), Value::from("object"));
    }
    schema
}

/// Refuses `name` when no vendor accepts it (RFC §6.7).
fn checked(name: &str) -> Result<(), ValueError> {
    if some_vendor_accepts_tool_name(name) {
        Ok(())
    } else {
        Err(ValueError::ToolName {
            name: name.to_owned(),
        })
    }
}

/// Reads a tool's name, refusing one no vendor accepts.
fn accepted_name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let name = String::deserialize(deserializer)?;
    checked(&name).map_err(serde::de::Error::custom)?;
    Ok(name)
}

/// How a tool call ended (RFC §5.3, §6.4): Python's `ToolCallOutcome`, one
/// of `served`, `refused`, `failed`, `blocked`, `unserved` and `cancelled`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolCallOutcome {
    /// A handler served the call.
    Served,
    /// The call was refused before execution.
    Refused,
    /// The call failed during execution.
    Failed,
    /// A hook blocked the call's result.
    Blocked,
    /// Nothing served the call.
    Unserved,
    /// The call was cut before its outcome.
    Cancelled,
}

impl<'de> Deserialize<'de> for ToolCallOutcome {
    /// Reads an outcome from its name, a string.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        read_name(
            deserializer,
            "one of the outcomes `served`, `refused`, `failed`, `blocked`, `unserved` and \
             `cancelled`",
            |name| match name {
                "served" => Some(Self::Served),
                "refused" => Some(Self::Refused),
                "failed" => Some(Self::Failed),
                "blocked" => Some(Self::Blocked),
                "unserved" => Some(Self::Unserved),
                "cancelled" => Some(Self::Cancelled),
                _ => None,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_union_of_vendor_alphabets_is_available_to_tool_authors() {
        for name in ["lookup", "files.read", "ns:tool", "1abc"] {
            assert!(some_vendor_accepts_tool_name(name));
            assert!(AiTool::new(name, "test").is_ok());
        }
        for name in ["", "not allowed", "bad/path", "é"] {
            assert!(!some_vendor_accepts_tool_name(name));
            assert_eq!(
                AiTool::new(name, "test"),
                Err(ValueError::ToolName {
                    name: name.to_owned()
                })
            );
        }
    }

    // Python: AITool's name validator runs when the model is built, from
    // JSON too.
    #[test]
    fn a_name_no_vendor_accepts_is_refused_on_input() {
        let read = serde_json::from_value::<AiTool>(json!({
            "name": "not allowed",
            "description": "test",
        }));
        assert_eq!(
            read.unwrap_err().to_string(),
            "tool name 'not allowed' is accepted by no provider: \
             use letters, digits, '_', '.', ':' or '-'"
        );
    }

    #[test]
    fn absent_empty_and_untyped_schemas_get_an_object_root() {
        let schema = |value: Value| value.as_object().cloned().unwrap_or_default();
        let empty = json!({ "type": "object", "properties": {} });
        assert_eq!(json!(declared_parameters(None)), empty);
        assert_eq!(json!(declared_parameters(Some(&Map::new()))), empty);
        let untyped = schema(json!({ "type": null, "required": ["q"] }));
        assert_eq!(
            json!(declared_parameters(Some(&untyped))),
            json!({ "type": "object", "required": ["q"] })
        );
        let typed = schema(json!({ "type": "array", "items": { "type": "string" } }));
        assert_eq!(declared_parameters(Some(&typed)), typed);
    }

    #[test]
    fn a_declared_schema_is_the_caller_s_own_to_edit() {
        let mut tool = AiTool::new("lookup", "Find a value").unwrap();
        tool.parameters
            .insert("properties".into(), json!({ "q": ["original"] }));
        let mut declared = declared_parameters(Some(&tool.parameters));
        declared["properties"]["q"][0] = json!("edited");
        assert_eq!(tool.parameters["properties"], json!({ "q": ["original"] }));
        assert_eq!(declared["type"], "object");
    }

    #[test]
    fn parameters_that_are_no_object_are_refused_on_input() {
        let read = serde_json::from_value::<AiTool>(json!({
            "name": "lookup",
            "description": "test",
            "parameters": ["order"],
        }));
        assert!(read.is_err());
    }
}
