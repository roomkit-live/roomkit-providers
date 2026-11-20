//! Runtime-independent contracts for model providers.
//!
//! This crate is where a provider's request, response, stream events,
//! failures and model catalogue are defined, apart from any vendor. It has
//! no I/O and no async runtime, and depends on neither Python nor `RoomKit`:
//! provider implementations own their HTTP, and hosts own their
//! conversations.
//!
//! The crate defines today:
//!
//! - [`request`]: what one generation is asked, [`AiRequest`], and its
//!   messages, [`AiMessage`], with their content parts;
//! - [`tools`]: a tool as the model is told of it, [`AiTool`], what a call
//!   answered, [`ToolResult`], and how it ended, [`ToolCallOutcome`];
//! - [`ValueError`]: the error for a value the contract refuses.
//!
//! The response, the stream events, the provider failures and the model
//! catalogue are not defined yet.
//!
//! # Names and JSON forms
//!
//! A type that `RoomKit`'s Python API names keeps that name in Rust's casing:
//! Python's `AIMessage` is [`AiMessage`]. The request is [`AiRequest`]: the
//! fields a provider reads of Python's `AIContext`.
//!
//! Every type reads and writes the JSON its Python model gives with
//! `model_dump(mode="json")`: a part carries its `type`, a message's content
//! is a string or a list of parts, every field is written, a `null` and an
//! empty list or object included, and a field left out on input takes its
//! default. As in Python, the JSON form of a request never carries the
//! credential of one request: see [`AiRequest`].
//!
//! ```
//! use roomkit_providers_core::{AiMessage, AiRole};
//! use serde_json::json;
//!
//! let message = AiMessage::new(AiRole::User, "Where is my order?");
//! assert_eq!(
//!     serde_json::to_value(&message)?,
//!     json!({ "role": "user", "content": "Where is my order?", "metadata": {} })
//! );
//! # Ok::<(), serde_json::Error>(())
//! ```
//!
//! Input is stricter than Python's. A message whose `role` is not `system`,
//! `user`, `assistant` or `tool` is refused; so is a part without its `type`
//! inside a list of parts; and so is a value that pydantic would convert,
//! such as `"0.7"` for a number, `1.0` for an integer or `1` for a boolean.

#![forbid(unsafe_code)]

pub mod error;
pub mod request;
mod text_or_list;
pub mod tools;

pub use self::error::ValueError;
pub use self::request::{
    AiContentPart, AiImagePart, AiMessage, AiMessageContent, AiRequest, AiRole, AiTextPart,
    AiThinkingPart, AiToolCallPart, AiToolResultPart,
};
pub use self::tools::{
    AiTool, ToolCallOutcome, ToolResult, ToolResultPart, some_vendor_accepts_tool_name,
};

/// A JSON object, its members in the order they were written: Python's
/// `dict[str, Any]`, for a message's metadata, a call's arguments and a
/// request's data.
pub type Metadata = serde_json::Map<String, serde_json::Value>;
