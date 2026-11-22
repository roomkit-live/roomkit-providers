//! A conversation rendered as Chat Completions messages, by the renderer of
//! RoomKit's core, with `OpenAI`'s own rendering and the field a
//! configuration names for a model's earlier reasoning.

use roomkit_ai::{AiMessage, ChatDialect, ChatDialectOptions, ProviderError, ProviderErrorOptions};
use serde_json::Value;

/// The conversation as the endpoint reads it, the system prompt first.
/// `thinking_field` is the assistant message field a server reads a model's
/// earlier reasoning from; `None` sends none (RFC §6.4).
///
/// # Errors
///
/// The failure of an image the provider cannot send, and of a field that
/// is no name of a member.
pub(crate) fn chat_messages(
    messages: &[AiMessage],
    system_prompt: Option<&str>,
    thinking_field: Option<&str>,
    provider: &str,
) -> Result<Vec<Value>, ProviderError> {
    let options = ChatDialectOptions {
        thinking_field: thinking_field.map(str::to_owned),
        ..ChatDialectOptions::default()
    };
    let dialect = ChatDialect::new(options).map_err(|error| {
        let options = ProviderErrorOptions {
            provider: provider.into(),
            ..ProviderErrorOptions::default()
        };
        ProviderError::new(error.to_string(), options)
    })?;
    roomkit_ai::chat_messages(messages, system_prompt, &dialect, provider)
}
