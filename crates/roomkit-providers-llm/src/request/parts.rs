//! The parts of a message's content (RFC §6.4): text, image, reasoning, a
//! tool call and a tool's result.
//!
//! Each part writes its `type` first, as its Python model does. Read alone,
//! a part takes a missing `type` as its own and refuses any other; read
//! among others, its `type` says which part it is.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::Metadata;
use crate::strict::{Tagged, read_tagged};
use crate::tools::{ToolCallOutcome, ToolResult};

/// One part of a message: Python's `AITextPart`, `AIImagePart`,
/// `AIToolCallPart`, `AIToolResultPart` or `AIThinkingPart`, told apart by
/// its `type`.
#[derive(Clone, Debug, PartialEq)]
pub enum AiContentPart {
    /// A text.
    Text(AiTextPart),
    /// An image (RFC §6.4).
    Image(AiImagePart),
    /// A call the assistant made.
    ToolCall(AiToolCallPart),
    /// A tool's result.
    ToolResult(AiToolResultPart),
    /// A reasoning block the model wrote, replayed within its turn (RFC
    /// §6.4).
    Thinking(AiThinkingPart),
}

impl Serialize for AiContentPart {
    /// Writes the part as its own model does, `type` included.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Text(part) => part.serialize(serializer),
            Self::Image(part) => part.serialize(serializer),
            Self::ToolCall(part) => part.serialize(serializer),
            Self::ToolResult(part) => part.serialize(serializer),
            Self::Thinking(part) => part.serialize(serializer),
        }
    }
}

impl Tagged for AiContentPart {
    const EXPECTING: &'static str = "a content part, as a JSON object with its `type`";
    const TYPES: &'static [&'static str] =
        &["text", "image", "tool_call", "tool_result", "thinking"];

    fn read<'de, D: Deserializer<'de>>(tag: &str, rest: D) -> Option<Result<Self, D::Error>> {
        Some(match tag {
            "text" => AiTextPart::deserialize(rest).map(Self::Text),
            "image" => AiImagePart::deserialize(rest).map(Self::Image),
            "tool_call" => AiToolCallPart::deserialize(rest).map(Self::ToolCall),
            "tool_result" => AiToolResultPart::deserialize(rest).map(Self::ToolResult),
            "thinking" => AiThinkingPart::deserialize(rest).map(Self::Thinking),
            _ => return None,
        })
    }
}

impl<'de> Deserialize<'de> for AiContentPart {
    /// Reads a part from an object, its `type` a string saying which.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        read_tagged(deserializer)
    }
}

/// A text part: Python's `AITextPart(*, text)`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename = "text")]
pub struct AiTextPart {
    /// The text.
    pub text: String,
}

impl AiTextPart {
    /// The part saying `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

/// An image part: Python's `AIImagePart(*, url, mime_type=None)`, a data or
/// HTTP URL the host resolved (RFC §6.4).
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename = "image")]
pub struct AiImagePart {
    /// Where the image is: a `data:` or `https:` URL.
    pub url: String,
    /// Its media type, when known.
    pub mime_type: Option<String>,
}

impl AiImagePart {
    /// The image at `url`, its media type unknown.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            mime_type: None,
        }
    }
}

impl std::fmt::Debug for AiImagePart {
    /// The URL is left out: a data URL holds the whole image.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AiImagePart")
            .field("mime_type", &self.mime_type)
            .finish_non_exhaustive()
    }
}

/// A reasoning block in the conversation (RFC §6.4): Python's
/// `AIThinkingPart(*, thinking, signature=None, redacted=None)`, kept across a
/// turn's rounds, as a vendor that signs its blocks requires.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename = "thinking")]
pub struct AiThinkingPart {
    /// The reasoning text the model wrote.
    pub thinking: String,
    /// The vendor's opaque token for the block.
    pub signature: Option<String>,
    /// The opaque data of a block the vendor redacted, replayed as received;
    /// `thinking` is then empty.
    pub redacted: Option<String>,
}

impl AiThinkingPart {
    /// The block saying `thinking`.
    pub fn new(thinking: impl Into<String>) -> Self {
        Self {
            thinking: thinking.into(),
            ..Self::default()
        }
    }
}

/// A call the assistant made: Python's `AIToolCallPart(*, id, name,
/// arguments={}, metadata={})`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename = "tool_call")]
pub struct AiToolCallPart {
    /// The call's id.
    pub id: String,
    /// The tool called.
    pub name: String,
    /// Its arguments.
    pub arguments: Metadata,
    /// Provider-owned data of the call.
    pub metadata: Metadata,
}

impl AiToolCallPart {
    /// The call `id` of the tool `name`, with no arguments.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments: Metadata::new(),
            metadata: Metadata::new(),
        }
    }
}

/// A tool's result: Python's `AIToolResultPart(*, tool_call_id, name,
/// result, is_error=False, structured_content=None, outcome=None, references=[])`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename = "tool_result")]
pub struct AiToolResultPart {
    /// The call the result answers.
    pub tool_call_id: String,
    /// The tool that answered.
    pub name: String,
    /// What the model reads of the result: a text, or its text and image
    /// parts in order (RFC §6.4).
    pub result: ToolResult,
    /// Whether the call failed.
    pub is_error: bool,
    /// The tool's structured copy, separate from its model-facing text
    /// (RFC §6.7). Carried within a turn, never rendered to the provider wire.
    pub structured_content: Option<Metadata>,
    /// How the call ended (RFC §6.7), carried without inferring it from
    /// result text. A result may have no recorded outcome.
    pub outcome: Option<ToolCallOutcome>,
    /// Deferred tools made callable by this result, in declaration order.
    pub references: Vec<String>,
}

impl AiToolResultPart {
    /// The result `result` of the call `tool_call_id` to `name`, not an
    /// error.
    pub fn new(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        result: impl Into<ToolResult>,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            name: name.into(),
            result: result.into(),
            is_error: false,
            structured_content: None,
            outcome: None,
            references: Vec::new(),
        }
    }

    /// The result as a provider that takes no image in a tool's result
    /// reads it: its texts and an `[image]` mark for each image, one per
    /// line. Python's `as_text()`.
    #[must_use]
    pub fn as_text(&self) -> String {
        self.result.as_text()
    }

    /// The text the tool message keeps and the images a provider sends
    /// after it, on a user message: the texts joined, one per line, or
    /// `[see image below]` for images alone. Python's
    /// `split_for_message()`.
    #[must_use]
    pub fn split_for_message(&self) -> (String, Vec<AiImagePart>) {
        self.result.split_for_message()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::tools::ToolResultPart;

    // Python: providers/ai/base.py::AIToolResultPart defaults and rendering
    // helpers.
    #[test]
    fn a_result_keeps_the_copy_separate_from_its_text() {
        let mut part = AiToolResultPart::new("c1", "lookup", "body");
        assert_eq!(part.structured_content, None);
        assert_eq!(part.outcome, None);
        part.outcome = Some(ToolCallOutcome::Served);
        part.structured_content = json!({"page": 3}).as_object().cloned();
        assert_eq!(part.as_text(), "body");
        assert_eq!(part.split_for_message(), ("body".to_owned(), Vec::new()));
    }

    #[test]
    fn an_image_s_debug_form_leaves_its_url_out() {
        let image = AiImagePart {
            url: "data:image/png;base64,iVBORw0KGgo=".into(),
            mime_type: Some("image/png".into()),
        };
        let shown = format!("{image:?}");
        assert!(!shown.contains("iVBORw0KGgo"), "{shown}");
        assert!(shown.contains("image/png"), "{shown}");
    }

    // Python: AITextPart.model_validate({"text": "hi"}) takes its own type;
    // {"type": "image", "text": "hi"} is refused.
    #[test]
    fn a_part_read_alone_takes_its_own_type_and_refuses_another() {
        assert_eq!(
            serde_json::from_value::<AiTextPart>(json!({ "text": "hi" })).unwrap(),
            AiTextPart::new("hi")
        );
        assert!(
            serde_json::from_value::<AiTextPart>(json!({ "type": "image", "text": "hi" })).is_err()
        );
        assert!(
            serde_json::from_value::<AiTextPart>(json!({ "type": null, "text": "hi" })).is_err()
        );
        assert!(
            serde_json::from_value::<AiImagePart>(
                json!({ "type": "text", "url": "https://example.com/a.png" })
            )
            .is_err()
        );
    }

    #[test]
    fn a_part_among_others_is_told_apart_by_its_type() {
        let read = |value| serde_json::from_value::<AiContentPart>(value);
        assert_eq!(
            read(json!({ "type": "thinking", "thinking": "first, look it up" })).unwrap(),
            AiContentPart::Thinking(AiThinkingPart::new("first, look it up"))
        );
        assert_eq!(
            read(json!({ "type": "tool_call", "id": "c1", "name": "lookup" })).unwrap(),
            AiContentPart::ToolCall(AiToolCallPart::new("c1", "lookup"))
        );
        // A field the part does not have is ignored, as pydantic ignores it.
        assert_eq!(
            read(json!({ "type": "text", "text": "hi", "extra": 1 })).unwrap(),
            AiContentPart::Text(AiTextPart::new("hi"))
        );
        for refused in [
            json!({ "type": "audio", "url": "https://example.com/a.wav" }),
            json!({ "text": "no type" }),
            json!({ "type": "image" }),
            json!("text"),
        ] {
            assert!(read(refused.clone()).is_err(), "{refused}");
        }
        assert!(
            serde_json::from_value::<ToolResultPart>(
                json!({ "type": "tool_call", "id": "c1", "name": "x" })
            )
            .is_err()
        );
    }
}
