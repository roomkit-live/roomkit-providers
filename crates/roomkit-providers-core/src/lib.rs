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
//!   messages, [`AiMessage`], with their content parts and the readings of
//!   an image part;
//! - [`tools`]: a tool as the model is told of it, [`AiTool`], what a call
//!   answered, [`ToolResult`], and how it ended, [`ToolCallOutcome`];
//! - [`response`]: what a generation answers, [`AiResponse`], the calls it
//!   makes, [`AiToolCall`], and the events of a structured stream,
//!   [`StreamEvent`];
//! - [`catalog`]: what a provider knows of its models, [`ModelInfo`], and
//!   their list price, [`ModelPricing`];
//! - [`error`]: a provider's call that failed, [`ProviderError`], and
//!   [`ValueError`], the error for a value the contract refuses.
//!
//! The provider trait itself, with its streams and the checks of a response
//! schema, is not defined yet.
//!
//! # Names and JSON forms
//!
//! A type that `RoomKit`'s Python API names keeps that name in Rust's casing:
//! Python's `AIMessage` is [`AiMessage`]. The request is [`AiRequest`]: the
//! fields a provider reads of Python's `AIContext`.
//!
//! Every type that is a model in Python reads and writes the JSON its
//! Python model gives with `model_dump(mode="json")`: a part and a stream
//! event carry their `type`, a message's content is a string or a list of
//! parts, every field is written, a `null` and an empty list or object
//! included, and a field left out on input takes its default. As in Python,
//! the JSON form of a request never carries the credential of one request:
//! see [`AiRequest`]. [`ProviderError`] is an exception in Python and has no
//! JSON form.
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
//! inside a list of parts, or an event without its `type` read as a
//! [`StreamEvent`]; and so is a value that pydantic would convert, such as
//! `"0.7"` for a number, `1.0` for an integer, `1` for a boolean or a
//! timestamp for a day.

#![forbid(unsafe_code)]

pub mod catalog;
mod data_uri;
pub mod error;
pub mod request;
pub mod response;
mod text_or_list;
pub mod tools;
mod type_tag;
mod usage;

#[doc(hidden)]
pub use self::catalog::listing;
pub use self::catalog::{
    Date, ModelInfo, ModelPricing, SPEECH_CAPABILITY, TRANSCRIPTION_CAPABILITY, speech_tags,
    with_speech_tags,
};
pub use self::error::{
    ProviderError, ProviderErrorOptions, ValueError, is_context_overflow_message,
};
pub use self::request::{
    AiContentPart, AiImagePart, AiMessage, AiMessageContent, AiRequest, AiRole, AiTextPart,
    AiThinkingPart, AiToolCallPart, AiToolResultPart, image_part_base64, image_part_payload,
    image_part_uri,
};
pub use self::response::{
    AiResponse, AiToolCall, CallIds, ServedCall, StreamDone, StreamEvent, StreamTextDelta,
    StreamThinkingDelta, StreamToolCall, StreamToolCallDelta, StreamUsage, is_truncation,
    stream_call_of, thinking_parts_of, tool_arguments, tool_call_of,
};
pub use self::tools::{
    AiTool, ToolCallOutcome, ToolResult, ToolResultPart, some_vendor_accepts_tool_name,
};

/// A JSON object, its members in the order they were written: Python's
/// `dict[str, Any]`, for a message's metadata, a call's arguments and a
/// request's data.
pub type Metadata = serde_json::Map<String, serde_json::Value>;
