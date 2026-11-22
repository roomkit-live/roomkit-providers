//! The calls of one streamed message, folded from its `tool_use` blocks:
//! Python's `ToolUseBlocks`.
//!
//! A block's call is told when the block opens, its arguments stream as
//! fragments, and it is handed when the block closes. Every composition
//! event names the call by the id it ends with, and a call keeps that id
//! and the arguments that streamed even when the message ended before its
//! block closed (RFC §6.4). Each call is handed once.
//!
//! What is kept of the calls is bounded: how many a message makes, and the
//! id, the name and the arguments of each.

use roomkit_ai::{
    AiToolCall, CallIds, StreamToolCall, StreamToolCallDelta, tool_arguments, unreadable_arguments,
};
use roomkit_providers_client::ANSWER_LIMIT;
use serde_json::Value;

use super::unreadable;
use crate::failure::{Failure, said_something};

/// The most calls one message makes.
pub(super) const CALLS_LIMIT: usize = 1024;

/// The most bytes of a call's id, and of its tool's name.
pub(super) const NAME_LIMIT: usize = 1024;

/// The most bytes of one call's arguments: what an answer read whole may
/// weigh.
pub(super) const ARGUMENTS_LIMIT: usize = ANSWER_LIMIT;

/// A call whose block is open: what its composition told so far.
#[derive(Debug)]
struct OpenCall {
    index: u32,
    id: String,
    name: String,
    arguments: String,
}

/// The calls of one message.
#[derive(Debug, Default)]
pub(super) struct Calls {
    /// Those whose block is open, in the order they opened.
    open: Vec<OpenCall>,
    /// How many the message made so far.
    made: usize,
    ids: CallIds,
}

/// A call's id as its text, as Python's `CallIds` reads what the server
/// wrote: none for an id Python reads as false (null, zero, empty), a
/// number as its digits, `True`, and a list or an object as its JSON.
fn id_text(id: Option<&Value>) -> String {
    match id.filter(|id| said_something(id)) {
        Some(Value::String(id)) => id.clone(),
        Some(Value::Bool(_)) => "True".to_owned(),
        Some(id @ (Value::Number(_) | Value::Array(_) | Value::Object(_))) => id.to_string(),
        Some(Value::Null) | None => String::new(),
    }
}

impl Calls {
    /// The call the block `block` opens at `index`, told at once, before
    /// any of its arguments, under an id of its own: the server's when no
    /// earlier call of the message took it.
    ///
    /// # Errors
    ///
    /// The failure of a block that names no tool, and of a message past
    /// what is kept of its calls.
    pub(super) fn open(
        &mut self,
        index: u32,
        block: &Value,
    ) -> Result<StreamToolCallDelta, Failure> {
        let name = block.get("name").and_then(Value::as_str);
        let name = name.ok_or_else(|| unreadable("a call names no tool"))?;
        let server_id = id_text(block.get("id"));
        if name.len() > NAME_LIMIT || server_id.len() > NAME_LIMIT {
            return Err(unreadable(
                "the id or the name of a call is longer than 1024 bytes",
            ));
        }
        if self.made >= CALLS_LIMIT {
            return Err(unreadable("the answer holds more than 1024 calls"));
        }
        self.made += 1;
        let id = self.ids.call(&server_id, name);
        let told = StreamToolCallDelta {
            index,
            ..StreamToolCallDelta::new(id.as_str(), name)
        };
        self.open.push(OpenCall {
            index,
            id,
            name: name.to_owned(),
            arguments: String::new(),
        });
        Ok(told)
    }

    /// Adds `fragment` to the arguments of the call open at `index`.
    ///
    /// # Errors
    ///
    /// The failure of arguments past their bound.
    pub(super) fn fragment(
        &mut self,
        index: u32,
        fragment: &str,
    ) -> Result<StreamToolCallDelta, Failure> {
        let call = self.open.iter_mut().find(|call| call.index == index);
        let call = call.ok_or_else(|| unreadable("a delta names no open content block"))?;
        if call.arguments.len().saturating_add(fragment.len()) > ARGUMENTS_LIMIT {
            return Err(unreadable(
                "the arguments of a call are longer than 4194304 bytes",
            ));
        }
        call.arguments.push_str(fragment);
        Ok(StreamToolCallDelta {
            index,
            arguments_delta: fragment.to_owned(),
            ..StreamToolCallDelta::new(call.id.as_str(), call.name.as_str())
        })
    }

    /// The call whose block at `index` closes. A closed block was written
    /// whole: arguments that do not read make a call that does not run,
    /// never one the model garbled.
    pub(super) fn close(&mut self, index: u32) -> Option<StreamToolCall> {
        let at = self.open.iter().position(|call| call.index == index)?;
        let call = self.open.remove(at);
        let mut handed = StreamToolCall::new(call.id, call.name);
        handed.arguments = tool_arguments(&call.arguments);
        handed.partial = unreadable_arguments(&Value::String(call.arguments));
        Some(handed)
    }

    /// The calls the stream left open, once its message is over for
    /// `finish_reason`, in the order they opened. Anthropic closes no block
    /// a cut stops: each may be the one the response was cut over, and is
    /// read so.
    pub(super) fn left_open(
        self,
        finish_reason: Option<&str>,
    ) -> impl Iterator<Item = StreamToolCall> {
        let read = move |call: OpenCall| {
            AiToolCall::read(call.id, call.name, &call.arguments, finish_reason, true)
        };
        self.open.into_iter().map(read).map(StreamToolCall::from)
    }
}
