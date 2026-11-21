//! What a tool's call answered (RFC §21.4): a text, or content parts, a text
//! and the images a model that sees reads (RFC §6.4).

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::request::{AiImagePart, AiTextPart};
use crate::strict::{Tagged, read_tagged};
use crate::text_or_list::TextOrList;

/// What stands for an image in a result's text: Python's
/// `AIToolResultPart.as_text`.
const IMAGE_MARK: &str = "[image]";

/// What a tool message says of a result of images alone, its images going
/// after it: Python's `split_for_message`.
const IMAGE_BELOW: &str = "[see image below]";

/// What a tool's call answered with (RFC §21.4): Python's `ToolResult`,
/// `str | list[AITextPart | AIImagePart]`. A text the model reads as it
/// is, or parts in order, a text and its images. Its JSON form is a string
/// or a list of parts.
///
/// ```
/// use roomkit_providers_llm::{
///     AiImagePart, AiTextPart, AiToolResultPart, ToolResult, ToolResultPart,
/// };
///
/// let screenshot = ToolResult::Parts(vec![
///     ToolResultPart::Text(AiTextPart::new("the page")),
///     ToolResultPart::Image(AiImagePart {
///         url: "data:image/png;base64,iVBORw0KGgo=".into(),
///         mime_type: Some("image/png".into()),
///     }),
/// ]);
/// let result = AiToolResultPart::new("call-1", "screenshot", screenshot);
/// assert_eq!(result.as_text(), "the page\n[image]");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum ToolResult {
    /// Text.
    Text(String),
    /// Parts in order: Python's `list[AITextPart | AIImagePart]`.
    Parts(Vec<ToolResultPart>),
}

/// One part of a tool's result: Python's `AITextPart | AIImagePart`, told
/// apart by its `type`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolResultPart {
    /// A text.
    Text(AiTextPart),
    /// An image (RFC §6.4).
    Image(AiImagePart),
}

impl Tagged for ToolResultPart {
    const EXPECTING: &'static str = "a text or image part, as a JSON object with its `type`";
    const TYPES: &'static [&'static str] = &["text", "image"];

    fn read<'de, D: Deserializer<'de>>(tag: &str, rest: D) -> Option<Result<Self, D::Error>> {
        Some(match tag {
            "text" => AiTextPart::deserialize(rest).map(Self::Text),
            "image" => AiImagePart::deserialize(rest).map(Self::Image),
            _ => return None,
        })
    }
}

impl<'de> Deserialize<'de> for ToolResultPart {
    /// Reads a part from an object, its `type` a string saying which.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        read_tagged(deserializer)
    }
}

impl Serialize for ToolResultPart {
    /// Writes the part as its own model does, `type` included.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Text(part) => part.serialize(serializer),
            Self::Image(part) => part.serialize(serializer),
        }
    }
}

impl ToolResult {
    /// The result as a reader that takes no image reads it: a text as it
    /// is, parts as their texts and an `[image]` mark for each image, one
    /// per line. Python's `AIToolResultPart.as_text`.
    #[must_use]
    pub(crate) fn as_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Parts(parts) => parts
                .iter()
                .map(|part| match part {
                    ToolResultPart::Text(part) => part.text.as_str(),
                    ToolResultPart::Image(_) => IMAGE_MARK,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// The text a tool message keeps and the images that go after it:
    /// the texts joined, one per line, or `[see image below]` for images
    /// alone. A text has no image. Python's
    /// `AIToolResultPart.split_for_message`.
    #[must_use]
    pub(crate) fn split_for_message(&self) -> (String, Vec<AiImagePart>) {
        let parts = match self {
            Self::Text(text) => return (text.clone(), Vec::new()),
            Self::Parts(parts) => parts,
        };
        let mut texts = Vec::new();
        let mut images = Vec::new();
        for part in parts {
            match part {
                ToolResultPart::Text(part) => texts.push(part.text.as_str()),
                ToolResultPart::Image(image) => images.push(image.clone()),
            }
        }
        let text = match (texts.is_empty(), images.is_empty()) {
            (true, false) => IMAGE_BELOW.to_owned(),
            _ => texts.join("\n"),
        };
        (text, images)
    }
}

impl From<String> for ToolResult {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for ToolResult {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<Vec<ToolResultPart>> for ToolResult {
    fn from(parts: Vec<ToolResultPart>) -> Self {
        Self::Parts(parts)
    }
}

impl<'de> Deserialize<'de> for ToolResult {
    /// Reads a string as a text and a list as parts; anything else is
    /// refused, as Python's model refuses it.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let read =
            TextOrList::deserialize(deserializer, "a text or a list of text and image parts")?;
        Ok(match read {
            TextOrList::Text(text) => Self::Text(text),
            TextOrList::List(parts) => Self::Parts(parts),
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ToolResult, ToolResultPart};
    use crate::request::{AiImagePart, AiTextPart};

    fn image(url: &str) -> ToolResultPart {
        ToolResultPart::Image(AiImagePart::new(url))
    }

    // Python: tests/test_providers/test_anthropic.py::test_as_text_flattens_parts
    // and test_split_for_message_separates_text_and_images.
    #[test]
    fn a_result_splits_into_its_text_and_its_images() {
        let both = ToolResult::Parts(vec![
            ToolResultPart::Text(AiTextPart::new("a")),
            image("data:image/png;base64,AAAA"),
            ToolResultPart::Text(AiTextPart::new("b")),
        ]);
        assert_eq!(both.as_text(), "a\n[image]\nb");
        let (text, images) = both.split_for_message();
        assert_eq!((text.as_str(), images.len()), ("a\nb", 1));
        let alone = ToolResult::Parts(vec![image("data:image/png;base64,AAAA")]);
        assert_eq!(alone.split_for_message().0, "[see image below]");
        assert_eq!(
            ToolResult::from("t").split_for_message(),
            ("t".into(), vec![])
        );
        let none = ToolResult::Parts(vec![]);
        assert_eq!(none.as_text(), "");
        assert_eq!(none.split_for_message(), (String::new(), vec![]));
    }

    // Python: AIToolResultPart(result=...) refuses a mapping, a number and a
    // list holding anything other than a text or an image part.
    #[test]
    fn a_result_that_is_no_text_and_no_list_of_parts_is_refused_on_input() {
        for value in [
            json!({ "status": "shipped" }),
            json!(3),
            json!(null),
            json!([{ "id": 1 }]),
            json!([{ "type": "thinking", "thinking": "x" }]),
        ] {
            assert!(
                serde_json::from_value::<ToolResult>(value.clone()).is_err(),
                "{value}"
            );
        }
    }
}
