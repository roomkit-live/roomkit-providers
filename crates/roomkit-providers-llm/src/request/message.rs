//! One message of the conversation a request carries.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::parts::{AiContentPart, AiToolCallPart, AiToolResultPart};
use crate::Metadata;
use crate::strict::read_name;
use crate::text_or_list::TextOrList;

/// One message of the conversation: Python's `AIMessage(*, role, content,
/// metadata={})`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AiMessage {
    /// Whose message it is.
    pub role: AiRole,
    /// What it says: a text, or its parts.
    pub content: AiMessageContent,
    /// Provider-owned data carried within the current tool loop (RFC §6.7),
    /// apart from the visible text and the conversation's history.
    pub metadata: Metadata,
}

impl AiMessage {
    /// A message of `role` saying `content`, with no metadata.
    pub fn new(role: AiRole, content: impl Into<AiMessageContent>) -> Self {
        Self {
            role,
            content: content.into(),
            metadata: Metadata::new(),
        }
    }

    /// The text the message says, its text parts and its tool results
    /// joined. For a test.
    #[doc(hidden)]
    pub fn text(&self) -> String {
        let results = self.tool_results();
        if results.is_empty() {
            return self.content.text();
        }
        results.iter().map(|result| result.as_text()).collect()
    }

    /// The calls the message makes, in order. For a test.
    #[doc(hidden)]
    pub fn tool_calls(&self) -> Vec<&AiToolCallPart> {
        self.parts()
            .filter_map(|part| match part {
                AiContentPart::ToolCall(call) => Some(call),
                AiContentPart::Text(_)
                | AiContentPart::Image(_)
                | AiContentPart::Thinking(_)
                | AiContentPart::ToolResult(_) => None,
            })
            .collect()
    }

    /// The result the message carries, when it is a tool's: the first of a
    /// round's. For a test.
    #[doc(hidden)]
    pub fn tool_result(&self) -> Option<&AiToolResultPart> {
        self.tool_results().into_iter().next()
    }

    /// The results the message carries, a round's in call order, when it
    /// is a tool's. For a test.
    #[doc(hidden)]
    pub fn tool_results(&self) -> Vec<&AiToolResultPart> {
        self.parts()
            .filter_map(|part| match part {
                AiContentPart::ToolResult(result) => Some(result),
                AiContentPart::Text(_)
                | AiContentPart::Image(_)
                | AiContentPart::Thinking(_)
                | AiContentPart::ToolCall(_) => None,
            })
            .collect()
    }

    /// The message's parts; none for a text alone.
    fn parts(&self) -> impl Iterator<Item = &AiContentPart> {
        match &self.content {
            AiMessageContent::Text(_) => [].iter(),
            AiMessageContent::Parts(parts) => parts.iter(),
        }
    }
}

/// Whose a message is: Python's `role`, `system`, `user`, `assistant` or
/// `tool`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AiRole {
    /// The application's instructions.
    System,
    /// Someone other than the assistant.
    User,
    /// The assistant itself.
    Assistant,
    /// A tool's result.
    Tool,
}

impl AiRole {
    /// The role as Python writes it.
    #[doc(hidden)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

/// Every role.
const ROLES: [AiRole; 4] = [
    AiRole::System,
    AiRole::User,
    AiRole::Assistant,
    AiRole::Tool,
];

impl Serialize for AiRole {
    /// Writes the role as Python writes it.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AiRole {
    /// Reads a role from its name, a string.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        read_name(
            deserializer,
            "one of the roles `system`, `user`, `assistant` and `tool`",
            |name| ROLES.into_iter().find(|role| role.as_str() == name),
        )
    }
}

/// What a message says: Python's `content`, a text or a list of parts.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum AiMessageContent {
    /// A text alone.
    Text(String),
    /// Its parts, in order.
    Parts(Vec<AiContentPart>),
}

impl AiMessageContent {
    /// The text the content says, its text parts joined.
    #[doc(hidden)]
    pub fn text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Parts(parts) => parts
                .iter()
                .filter_map(|part| match part {
                    AiContentPart::Text(part) => Some(part.text.as_str()),
                    AiContentPart::Image(_)
                    | AiContentPart::Thinking(_)
                    | AiContentPart::ToolCall(_)
                    | AiContentPart::ToolResult(_) => None,
                })
                .collect(),
        }
    }
}

impl<'de> Deserialize<'de> for AiMessageContent {
    /// Reads a string as a text and a list as parts.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let read = TextOrList::deserialize(deserializer, "a text or a list of content parts")?;
        Ok(match read {
            TextOrList::Text(text) => Self::Text(text),
            TextOrList::List(parts) => Self::Parts(parts),
        })
    }
}

impl From<String> for AiMessageContent {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for AiMessageContent {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<Vec<AiContentPart>> for AiMessageContent {
    fn from(parts: Vec<AiContentPart>) -> Self {
        Self::Parts(parts)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::request::{AiImagePart, AiTextPart};

    #[test]
    fn a_message_s_readers_follow_its_parts() {
        let text = AiMessage::new(AiRole::User, "hello");
        assert_eq!(text.text(), "hello");
        assert!(text.tool_calls().is_empty() && text.tool_result().is_none());

        let asking = AiMessage::new(
            AiRole::Assistant,
            vec![
                AiContentPart::Text(AiTextPart::new("Let me ")),
                AiContentPart::Image(AiImagePart::new("https://example.com/a.png")),
                AiContentPart::Text(AiTextPart::new("check.")),
                AiContentPart::ToolCall(AiToolCallPart::new("c1", "lookup")),
                AiContentPart::ToolCall(AiToolCallPart::new("c2", "search")),
            ],
        );
        assert_eq!(asking.text(), "Let me check.");
        let called: Vec<&str> = asking
            .tool_calls()
            .iter()
            .map(|call| call.name.as_str())
            .collect();
        assert_eq!(called, ["lookup", "search"]);

        let answering = AiMessage::new(
            AiRole::Tool,
            vec![
                AiContentPart::ToolResult(AiToolResultPart::new("c1", "lookup", "shipped")),
                AiContentPart::ToolResult(AiToolResultPart::new("c2", "search", "none")),
            ],
        );
        assert_eq!(answering.text(), "shippednone");
        assert_eq!(answering.tool_results().len(), 2);
        assert_eq!(
            answering.tool_result().map(|result| result.name.as_str()),
            Some("lookup")
        );
    }

    #[test]
    fn a_role_is_written_as_python_writes_it() {
        for role in [
            AiRole::System,
            AiRole::User,
            AiRole::Assistant,
            AiRole::Tool,
        ] {
            assert_eq!(serde_json::to_value(role).unwrap(), json!(role.as_str()));
            assert_eq!(
                serde_json::from_value::<AiRole>(json!(role.as_str())).unwrap(),
                role
            );
        }
        assert!(serde_json::from_value::<AiRole>(json!("developer")).is_err());
    }

    #[test]
    fn content_that_is_no_text_and_no_list_of_parts_is_refused_on_input() {
        for content in [
            json!(null),
            json!(3),
            json!({ "type": "text", "text": "x" }),
        ] {
            let read =
                serde_json::from_value::<AiMessage>(json!({ "role": "user", "content": content }));
            assert!(read.is_err(), "{content}");
        }
    }
}
