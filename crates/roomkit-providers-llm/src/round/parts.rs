//! The parts an assistant round is replayed as: its reasoning, its text and
//! its calls (RFC §6.4): Python's `round_parts` and `RoundTranscript`.
//!
//! One assembly for every reader of a round. The tool loop saw the round
//! stream, so it knows where each reasoning block came relative to the text
//! and the calls and replays them there. A caller of `generate()` has only
//! the response, which does not say where its blocks came: they go first,
//! then the text, then the calls.

use serde_json::Value;

use super::thinking_blocks::ThinkingBlocks;
use crate::error::ValueError;
use crate::request::{AiContentPart, AiTextPart, AiThinkingPart, AiToolCallPart};
use crate::response::{AiToolCall, StreamThinkingDelta};

/// The reserved property a call's intent rides in among its arguments (RFC
/// §6.4): Python's `INTENT_PROPERTY`.
const INTENT_PROPERTY: &str = "_roomkit_intent";

/// The history part of `call`: its intent, as normalized, back among its
/// arguments when it has one (RFC §6.4), its metadata (a thought signature)
/// kept. Python's `call_part`.
fn call_part(call: &AiToolCall) -> AiToolCallPart {
    let mut arguments = call.arguments.clone();
    if let Some(intent) = &call.intent {
        arguments.insert(INTENT_PROPERTY.to_owned(), Value::String(intent.clone()));
    }
    AiToolCallPart {
        arguments,
        metadata: call.metadata.clone(),
        ..AiToolCallPart::new(call.id.clone(), call.name.clone())
    }
}

/// A round whose reasoning has no known place: the reasoning, then the text,
/// then the calls, an [`AiToolCall`] or a
/// [`StreamToolCall`](crate::StreamToolCall) each, as Python's `RoundCall`.
/// An empty list names its type:
/// `round_parts::<AiToolCall>(&[], "text", &[])`.
#[must_use]
pub fn round_parts<C>(thinking: &[AiThinkingPart], text: &str, calls: &[C]) -> Vec<AiContentPart>
where
    C: Clone + Into<AiToolCall>,
{
    let mut parts: Vec<AiContentPart> = thinking
        .iter()
        .cloned()
        .map(AiContentPart::Thinking)
        .collect();
    if !text.is_empty() {
        parts.push(AiContentPart::Text(AiTextPart::new(text)));
    }
    parts.extend(
        calls
            .iter()
            .cloned()
            .map(|call| AiContentPart::ToolCall(call_part(&call.into()))),
    );
    parts
}

/// One entry of what a round said, in the order it came.
#[derive(Clone, Debug)]
enum Entry {
    Thinking(Option<u32>),
    Text(usize),
    Call(String),
}

/// What a round said, in the order it came: its reasoning blocks, the
/// stretches of text between them and its calls.
#[derive(Clone, Debug, Default)]
pub struct RoundTranscript {
    thinking: ThinkingBlocks,
    entries: Vec<Entry>,
    segments: Vec<String>,
}

impl RoundTranscript {
    /// Nothing said yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The round's reasoning blocks.
    #[must_use]
    pub fn thinking(&self) -> &ThinkingBlocks {
        &self.thinking
    }

    /// Everything the round said, its stretches joined.
    #[must_use]
    pub fn text(&self) -> String {
        self.segments.concat()
    }

    /// Keeps `delta`, in its block.
    pub fn add_thinking(&mut self, delta: &StreamThinkingDelta) {
        if self.thinking.add(delta) {
            self.entries.push(Entry::Thinking(delta.block));
        }
    }

    /// Keeps `text`, in the stretch under way or a new one.
    pub fn add_text(&mut self, text: &str) {
        if let (Some(Entry::Text(_)), Some(segment)) =
            (self.entries.last(), self.segments.last_mut())
        {
            segment.push_str(text);
            return;
        }
        self.entries.push(Entry::Text(self.segments.len()));
        self.segments.push(text.to_owned());
    }

    /// Keeps where the call `call_id` came.
    pub fn add_call(&mut self, call_id: impl Into<String>) {
        self.entries.push(Entry::Call(call_id.into()));
    }

    /// The round as the next one replays it, keeping `calls` of the ones it
    /// made. Reasoning a vendor sent in blocks goes back block by block, each
    /// where it came; a single block goes first, then the text, then the
    /// calls. An empty list names its type:
    /// `transcript.parts::<AiToolCall>(&[])`.
    ///
    /// # Errors
    ///
    /// [`ValueError::UnmadeCalls`] for a call in `calls` the round never
    /// made, where Python raises `RuntimeError`.
    pub fn parts<C>(&self, calls: &[C]) -> Result<Vec<AiContentPart>, ValueError>
    where
        C: Clone + Into<AiToolCall>,
    {
        if !self.thinking.keyed() {
            return Ok(round_parts(&self.thinking.parts(), &self.text(), calls));
        }
        let mut pending: Vec<AiToolCall> = calls.iter().cloned().map(Into::into).collect();
        let mut parts = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            match entry {
                Entry::Thinking(block) => {
                    parts.extend(self.thinking.part(*block).map(AiContentPart::Thinking));
                }
                Entry::Text(segment) => {
                    let text = self.segments.get(*segment).filter(|text| !text.is_empty());
                    parts.extend(text.map(|text| AiContentPart::Text(AiTextPart::new(text))));
                }
                Entry::Call(id) => {
                    if let Some(at) = pending.iter().position(|call| call.id == *id) {
                        let call = pending.remove(at);
                        parts.push(AiContentPart::ToolCall(call_part(&call)));
                    }
                }
            }
        }
        if pending.is_empty() {
            return Ok(parts);
        }
        let mut ids: Vec<String> = pending.into_iter().map(|call| call.id).collect();
        ids.sort_unstable();
        Err(ValueError::UnmadeCalls { ids })
    }
}

#[cfg(test)]
#[path = "parts_tests.rs"]
mod tests;
