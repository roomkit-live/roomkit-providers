//! A whole answer, folded from the events of its stream: Python's
//! `generate`, which reads the stream every call gives.

use roomkit_ai::{
    AiResponse, ProviderError, ProviderStream, StreamEvent, ThinkingBlocks, tool_call_of,
};
use roomkit_providers_client::items::next;

/// The response `events` stand for: its text, all its reasoning with the
/// last signature and each block to send back, its calls, and how it ended.
/// The composition of a call and the usage a message's start tells say
/// nothing more than the call and the done event that follow them.
///
/// # Errors
///
/// The failure the stream ended with: nothing of an answer that failed
/// under way is given.
pub(crate) async fn response(mut events: ProviderStream<'_>) -> Result<AiResponse, ProviderError> {
    let mut response = AiResponse::new("");
    let mut thinking = ThinkingBlocks::new();
    while let Some(event) = next(&mut events).await {
        match event? {
            StreamEvent::TextDelta(delta) => response.content.push_str(&delta.text),
            StreamEvent::ThinkingDelta(delta) => {
                thinking.add(&delta);
            }
            StreamEvent::ToolCall(call) => response.tool_calls.push(tool_call_of(&call)),
            StreamEvent::Done(done) => {
                response.finish_reason = done.finish_reason;
                response.usage = done.usage;
                response.metadata = done.metadata;
            }
            StreamEvent::ToolCallDelta(_) | StreamEvent::Usage(_) => {}
        }
    }
    response.thinking = thinking.seen().then(|| thinking.text());
    response.thinking_signature = thinking.last_signature().map(str::to_owned);
    response.thinking_parts = thinking.keyed().then(|| thinking.parts());
    Ok(response)
}
