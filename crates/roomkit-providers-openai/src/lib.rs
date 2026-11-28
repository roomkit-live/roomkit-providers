//! The `OpenAI` model provider of `RoomKit` Providers.
//!
//! - [`OpenAi`]: a provider for `OpenAI`'s Chat Completions and
//!   for the servers that speak it behind a base URL, Python's
//!   `OpenAIAIProvider`. It answers a request whole or as a stream, of
//!   events or of text, and lists the models its endpoint serves.
//! - [`OpenAiConfig`]: what the provider is configured with, the
//!   `OpenAIConfig` of `RoomKit`'s Python implementation: the same names,
//!   the same defaults and the same JSON form.
//! - [`DeepSeek`], [`Kimi`], [`MiniMax`], [`Xai`], [`Cerebras`], [`Qwen`],
//!   [`Meta`], [`OpenRouter`] and [`LiteLlm`]: the providers of nine vendors
//!   that speak Chat Completions with rules of their own (RFC §6.4, §6.7),
//!   each with its configuration: where a round's earlier reasoning goes
//!   back, how a reasoning setting is sent, the tool names the vendor
//!   takes. Each is the choice of its vendor's rules, on the vendor's API
//!   or behind a base URL; [`OpenAi`] sends no reasoning back unless its
//!   configuration names the field.
//!
//! - [`AzureAi`] and [`AzureAiConfig`]: an Azure resource's deployments,
//!   Python's `AzureAIProvider`: `OpenAI`'s wire at the deployment's path,
//!   the API version in the query and the key in Azure's `api-key` header,
//!   the endpoint a configuration states reduced to the resource's base.
//!
//! - [`Vllm`] and [`VllmConfig`]: a vLLM server's models, what Python's
//!   `create_vllm_provider` builds: `OpenAI`'s wire, a reasoning setting in
//!   the chat template's `chat_template_kwargs`, vLLM's sampling members in
//!   the body.
//!
//! A stream is pulled: nothing is sent before it is polled, and dropping it
//! stops the request and closes its connection. Every size a server
//! controls is bounded, and a stream that stalls ends at the
//! configuration's timeout: [`OpenAi`] says how.
//!
//! The provider is held to a table of cases captured on `RoomKit`'s Python
//! provider (`tests/fixtures/python_chat_cases.json`), each with the
//! request, the HTTP exchanges and what Python returned: the crate's tests
//! replay every case through a local fake of the endpoint, a stream read
//! by read, and name the few the provider answers otherwise, with the
//! reason. The crate's README lists them.
//!
//! It depends on `RoomKit`'s Rust core for what a provider is, and on no
//! Python.

#![forbid(unsafe_code)]

mod answer;
mod azure;
mod cerebras;
mod config;
mod deepseek;
mod dialect;
mod endpoint;
mod failure;
mod folding;
mod images;
mod kimi;
mod litellm;
mod meta;
mod minimax;
mod models;
mod names;
mod openrouter;
mod provider;
mod qwen;
#[cfg(test)]
mod replay;
mod request;
mod responses;
mod stream;
#[cfg(test)]
mod tests;
mod vendor;
mod vllm;
mod xai;

pub use azure::{AzureAi, AzureAiConfig};
pub use cerebras::{Cerebras, CerebrasConfig, CerebrasReasoningFormat};
pub use config::{OpenAiApi, OpenAiConfig};
pub use deepseek::{DeepSeek, DeepSeekConfig};
pub use images::{OpenAiImage, OpenAiImageConfig};
pub use kimi::{Kimi, KimiConfig};
pub use litellm::{LiteLlm, LiteLlmConfig};
pub use meta::{Meta, MetaConfig};
pub use minimax::{MiniMax, MiniMaxConfig};
pub use openrouter::{OpenRouter, OpenRouterConfig};
pub use provider::OpenAi;
#[cfg(feature = "replay")]
#[doc(hidden)]
pub use provider::ReplayWaits;
pub use qwen::{Qwen, QwenConfig};
pub use roomkit_providers_client::{TokenError, TokenFuture, TokenSource};
pub use vllm::{Vllm, VllmConfig};
pub use xai::{Xai, XaiConfig};
