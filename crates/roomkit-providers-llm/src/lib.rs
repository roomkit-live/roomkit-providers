//! Runtime-independent contracts for model providers.
//!
//! This crate is where a provider's request, response, stream events,
//! failures and model catalogue are defined, apart from any vendor. It has
//! no I/O and no async runtime, and depends on neither Python nor `RoomKit`:
//! provider implementations own their HTTP, and hosts own their
//! conversations.
//!
//! Section numbers in this documentation (`RFC §6.7`) refer to the
//! [`RoomKit` RFC](https://github.com/roomkit-live/roomkit-specs).
//!
//! The crate defines today:
//!
//! - [`provider`]: what a provider implements, [`AiProvider`], with its
//!   streams and the credential a host lends one request;
//! - [`request`]: what one generation is asked, [`AiRequest`], and its
//!   messages, [`AiMessage`], with their content parts and the readings of
//!   an image part;
//! - [`tools`]: a tool as the model is told of it, [`AiTool`], what a call
//!   answered, [`ToolResult`], and how it ended, [`ToolCallOutcome`];
//! - [`response`]: what a generation answers, [`AiResponse`], the calls it
//!   makes, [`AiToolCall`], and the events of a structured stream,
//!   [`StreamEvent`];
//! - [`round`]: how a round of a turn is read and replayed, its reasoning
//!   blocks ([`ThinkingBlocks`]), its parts ([`RoundTranscript`]) and the
//!   reading of a call's arguments;
//! - [`schema`]: the portable subset of JSON Schema a response schema stays
//!   within ([`check_portable_schema`]) and the failure of one
//!   ([`ResponseSchemaError`]);
//! - [`catalog`]: what a provider knows of its models, [`ModelInfo`], and
//!   their list price, [`ModelPricing`];
//! - [`error`]: a provider's call that failed, [`ProviderError`], and
//!   [`ValueError`], the error for a value the contract refuses;
//! - [`mock`]: a provider answering from a list, [`MockAiProvider`], for
//!   tests and examples.
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
//! see [`AiRequest`]. [`ProviderError`] and [`ResponseSchemaError`] are
//! exceptions in Python and have no JSON form.
//!
//! ```
//! use roomkit_providers_llm::{AiMessage, AiRole};
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
//! # What is read
//!
//! A model is read from a JSON object, and a name (a role, an outcome, a
//! `type`) from a JSON string. A list of a model's values, or a number or an
//! object where a name goes, is refused, as Python refuses it.
//!
//! Input is stricter than Python's. Each of these is refused here and taken
//! by Python:
//!
//! - a message whose `role` is not `system`, `user`, `assistant` or `tool`;
//! - a part without its `type` inside a list of parts, or an event without
//!   its `type` read as a [`StreamEvent`];
//! - a value pydantic would convert: `"0.7"` for a number, `1.0` for an
//!   integer, `1` for a boolean, `"12"` for a usage counter, a timestamp or
//!   a date with its time for a day;
//! - an integer outside its field's Rust type: a `max_tokens` of `-5` or of
//!   `4294967296`, an `index` or a `block` of `-1`, a `context_window` of
//!   `-1`.
//!
//! Other differences come from the JSON parser, `serde_json`, where Python
//! reads with its `json` module:
//!
//! - an integer past 64 bits is read as a float, which no integer field
//!   takes: `18446744073709551616` reads as `1.8446744073709552e19`;
//! - nesting of 128 levels or more is refused: a list in a list, 128 deep;
//! - a lone surrogate escape is refused: `"\ud800"`;
//! - a negative zero is a float: `-0` reads as `-0.0`;
//! - a number JSON does not name, or past a float's range, is refused:
//!   `NaN`, `Infinity`, `1e999`;
//! - a field written twice in a model's object is refused, where Python
//!   keeps the last: `{"temperature": 0.1, "temperature": 0.2}`. This holds
//!   when the text itself is read: a `serde_json::Value` has already kept
//!   the last.
//!
//! [`tool_arguments`] keeps a text the parser refuses whole under `raw`, as
//! it keeps any text that is no JSON object.

#![forbid(unsafe_code)]

pub mod catalog;
mod data_uri;
pub mod error;
pub mod mock;
pub mod provider;
mod python;
pub mod request;
pub mod response;
pub mod round;
pub mod schema;
mod stream;
mod strict;
#[cfg(test)]
mod testing;
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
#[doc(hidden)]
pub use self::data_uri::{DataUriError, parse_data_uri};
pub use self::error::{
    ProviderError, ProviderErrorOptions, ValueError, is_context_overflow_message,
};
pub use self::mock::{MockAiProvider, MockAiProviderOptions};
pub use self::provider::{
    API_KEY_METADATA_KEY, AiProvider, DynAiProvider, ProviderStream, TextStream, request_api_key,
};
// For a provider crate: a call's arguments as a wire writes them, and a
// value as the contract quotes it in a failure's text, which a provider
// must recognise a secret in.
#[doc(hidden)]
pub use self::python::{dumps, repr};
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
#[doc(hidden)]
pub use self::round::read_tool_calls;
pub use self::round::{
    CallCutOptions, CallGarbledOptions, CallPartialOptions, RoundTranscript, ThinkingBlocks,
    call_cut, call_garbled, call_partial, round_parts, unreadable_arguments,
};
pub use self::schema::{
    ResponseSchemaError, ResponseSchemaErrorOptions, ResponseSchemaFailure, SchemaMismatchOptions,
    check_portable_schema, schema_mismatch,
};
#[doc(hidden)]
pub use self::schema::{check_schema_answer, checked_stream, schema_for_generate};
// For a provider crate: the stream a provider's events come as, written as
// a producer that hands its items over, and the next item of a stream.
#[doc(hidden)]
pub use self::stream::{Items, Yielder, items, next};
#[doc(hidden)]
pub use self::strict::{ObjectOnly, read_name};
pub use self::tools::{
    AiTool, ToolCallOutcome, ToolResult, ToolResultPart, declared_parameters,
    some_vendor_accepts_tool_name,
};

/// A JSON object, its members in the order they were written: Python's
/// `dict[str, Any]`, for a message's metadata, a call's arguments and a
/// request's data.
pub type Metadata = serde_json::Map<String, serde_json::Value>;
