//! How a round of a turn is read and replayed (RFC §6.4): the parts an
//! assistant round goes back as, its reasoning blocks, and the one reading
//! of a call's arguments every provider applies.
//!
//! - [`round_parts`] and [`RoundTranscript`]: the parts a round is replayed
//!   as, its reasoning, its text and its calls.
//! - [`ThinkingBlocks`]: a response's reasoning blocks, folded from its
//!   streamed deltas.
//! - [`unreadable_arguments`], [`call_partial`], [`call_cut`] and
//!   [`call_garbled`], each with its options: whether a call's arguments
//!   read, and whether the call runs.

mod call_rules;
mod parts;
mod thinking_blocks;

pub(crate) use self::call_rules::is_malformed_call;
#[doc(hidden)]
pub use self::call_rules::read_tool_calls;
pub use self::call_rules::{
    CallCutOptions, CallGarbledOptions, CallPartialOptions, call_cut, call_garbled, call_partial,
    unreadable_arguments,
};
pub use self::parts::{RoundTranscript, round_parts};
pub use self::thinking_blocks::ThinkingBlocks;
