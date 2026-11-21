//! The provider a model is reached through (RFC §6.7): Python's
//! `roomkit.providers.ai.AIProvider`.
//!
//! - [`AiProvider`]: what a provider implements, `model_name` and `generate`
//!   required, the rest provided as Python provides it.
//! - [`DynAiProvider`]: a provider held as an object.
//! - [`ProviderStream`] and [`TextStream`]: the events and the text of one
//!   generation, as they come.
//! - [`API_KEY_METADATA_KEY`]: where a host leaves the credential it lends
//!   one request.

mod dynamic;

use std::future::Future;
use std::pin::Pin;

use futures_core::Stream;
use serde_json::Value;

pub use self::dynamic::DynAiProvider;
use crate::catalog::ModelInfo;
use crate::error::ProviderError;
use crate::request::AiRequest;
use crate::response::{AiResponse, StreamDone, StreamEvent, StreamTextDelta, response_events};
use crate::stream::{Yielder, items, next};

/// The key of [`AiRequest::metadata`] holding the credential to use for one
/// request: Python's `API_KEY_METADATA_KEY`.
///
/// A provider is built once and may serve every conversation of a host, so a
/// key fixed when it is built is everyone's. When the credential belongs to
/// the person making the request, the host leaves it under this key for the
/// one request. A provider that honours it falls back to its configured key
/// when the entry is absent.
pub const API_KEY_METADATA_KEY: &str = "api_key";

/// The credential `request` carries for itself, `None` to use the configured
/// one: Python's `request_api_key(context)`. Hidden, as Python's is no public
/// name: the provider crates of this repository read the credential with it. An absent entry, an empty string
/// and a value that is no string are all a host that lent no usable
/// credential.
///
/// A request that crossed a boundary as JSON does not hold the credential:
/// its JSON form writes a mask in its place ([`AiRequest`]), and this reads
/// the mask as any other string. A host that sends a request as JSON passes
/// the key apart.
///
/// ```
/// use roomkit_providers_llm::{API_KEY_METADATA_KEY, AiRequest, request_api_key};
///
/// let mut request = AiRequest::default();
/// assert_eq!(request_api_key(&request), None);
/// request
///     .metadata
///     .insert(API_KEY_METADATA_KEY.into(), "placeholder-not-a-key".into());
/// assert_eq!(request_api_key(&request), Some("placeholder-not-a-key"));
/// ```
#[doc(hidden)]
#[must_use]
pub fn request_api_key(request: &AiRequest) -> Option<&str> {
    request
        .metadata
        .get(API_KEY_METADATA_KEY)
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty())
}

/// The events of one generation, as a structured stream reads them: text
/// as it comes, the calls, then the end.
pub type ProviderStream<'a> =
    Pin<Box<dyn Stream<Item = Result<StreamEvent, ProviderError>> + Send + 'a>>;

/// The text of one generation, as it comes.
pub type TextStream<'a> = Pin<Box<dyn Stream<Item = Result<String, ProviderError>> + Send + 'a>>;

/// A model provider (RFC §6.7): Python's `AIProvider`. `model_name` and
/// `generate` are required; everything else is provided as Python
/// provides it.
///
/// A host reads one stream whatever the provider (RFC §6.4):
/// [`generate_structured_stream`](Self::generate_structured_stream), which
/// wraps [`generate`](Self::generate) for a provider that does not stream.
pub trait AiProvider: Send + Sync + 'static {
    /// The provider's name: Python's `name`, the provider's class name by
    /// default.
    fn name(&self) -> &str {
        short_type_name::<Self>()
    }

    /// The label the provider's failures carry as their
    /// [`provider`](ProviderError::provider): `openai` for `OpenAI`'s. What
    /// refuses a request on a provider's behalf, before the provider sees
    /// it, reads it here. Empty by default: a failure then names no
    /// provider.
    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "a provider may answer with what it holds"
    )]
    fn label(&self) -> &str {
        ""
    }

    /// The model the provider reaches, as configured: Python's
    /// `model_name`.
    fn model_name(&self) -> &str;

    /// Whether the configured model takes images (RFC §6.4).
    fn supports_vision(&self) -> bool {
        false
    }

    /// The provider's curated catalogue: what it knows of its models
    /// before any request, without a key (Python's classmethod
    /// `available_models()`). Not what the provider offers today, which
    /// only its endpoint can say; a model absent from it is ordinary. None
    /// by default.
    fn available_models() -> Vec<ModelInfo> {
        Vec::new()
    }

    /// The models the provider's API reports live: the discovery surface,
    /// the only answer that reflects the caller's own account, unlike
    /// [`available_models`](Self::available_models), which it falls back to
    /// for a provider whose API has no models endpoint (Python's
    /// `list_models()`).
    fn list_models(&self) -> impl Future<Output = Result<Vec<ModelInfo>, ProviderError>> + Send {
        std::future::ready(Ok(Self::available_models()))
    }

    /// The catalogue's entry for the configured model (Python's
    /// `catalog_entry()`), `None` for a model the catalogue does not carry,
    /// a custom or local one behind a base URL, or one newer than the
    /// catalogue.
    fn catalog_entry(&self) -> Option<ModelInfo> {
        let model = self.model_name();
        Self::available_models()
            .into_iter()
            .find(|entry| entry.id == model)
    }

    /// The configured model's input context window, in tokens, from the
    /// catalogue (Python's `context_window`): `None` for a model it does
    /// not carry, which a caller degrades on rather than assume one.
    fn context_window(&self) -> Option<u64> {
        self.catalog_entry().and_then(|entry| entry.context_window)
    }

    /// Whether [`generate_stream`](Self::generate_stream) streams.
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Whether [`generate_structured_stream`](Self::generate_structured_stream)
    /// streams, rather than wrap a whole response.
    fn supports_structured_streaming(&self) -> bool {
        false
    }

    /// Whether [`generate`](Self::generate) and the streams honour
    /// [`AiRequest::response_schema`] (RFC §6.7): Python's
    /// `supports_response_schema`, false by default. A provider default,
    /// not a fact about every model it reaches. The contract is the
    /// implementation's to keep: refuse a schema it cannot carry before
    /// sending anything, and never return an answer that does not satisfy
    /// it.
    fn supports_response_schema(&self) -> bool {
        false
    }

    /// Whether a response schema may ride a turn that also carries tools
    /// (RFC §6.7): the model then calls tools or answers in the schema,
    /// only the final answer checked. Python's
    /// `supports_response_schema_with_tools`, false by default, as wherever
    /// the constraint is a decoding grammar, which keeps the model from
    /// calling a tool.
    fn supports_response_schema_with_tools(&self) -> bool {
        false
    }

    /// Whether the model can hold a tool declared but unseen
    /// ([`AiTool::defer_loading`](crate::AiTool::defer_loading)) until a
    /// tool result references it (RFC §6.4). Python's
    /// `supports_deferred_tools`, false by default.
    fn supports_deferred_tools(&self) -> bool {
        false
    }

    /// One whole generation answering `request`.
    fn generate<'a>(
        &'a self,
        request: &'a AiRequest,
    ) -> impl Future<Output = Result<AiResponse, ProviderError>> + Send + 'a;

    /// The text of one generation answering `request`, as it comes. A
    /// provider that does not stream fails it, where Python's raises
    /// `NotImplementedError`.
    fn generate_stream<'a>(&'a self, _request: &'a AiRequest) -> TextStream<'a> {
        let failure = format!("{} does not support streaming generation", self.name());
        Box::pin(items(
            move |out: Yielder<Result<String, ProviderError>>| async move {
                out.give(Err(ProviderError::from(failure))).await;
            },
        ))
    }

    /// The events of one generation answering `request`. By default, as
    /// Python's: a turn without tools streams through
    /// [`generate_stream`](Self::generate_stream) when the provider streams
    /// text, ending `stop`; any other wraps [`generate`](Self::generate),
    /// everything it returned kept.
    fn generate_structured_stream<'a>(&'a self, request: &'a AiRequest) -> ProviderStream<'a> {
        if self.supports_streaming() && request.tools.is_empty() {
            let mut text = self.generate_stream(request);
            return Box::pin(items(
                move |out: Yielder<Result<StreamEvent, ProviderError>>| async move {
                    while let Some(delta) = next(&mut text).await {
                        match delta {
                            Ok(text) => {
                                let delta = StreamTextDelta { text };
                                out.give(Ok(StreamEvent::TextDelta(delta))).await;
                            }
                            Err(error) => return out.give(Err(error)).await,
                        }
                    }
                    // A text stream reports no usage and no stop reason.
                    let done = StreamDone {
                        finish_reason: Some("stop".to_owned()),
                        ..StreamDone::default()
                    };
                    out.give(Ok(StreamEvent::Done(done))).await;
                },
            ));
        }
        let response = self.generate(request);
        Box::pin(items(
            move |out: Yielder<Result<StreamEvent, ProviderError>>| async move {
                match response.await {
                    Ok(response) => {
                        for event in response_events(response) {
                            out.give(Ok(event)).await;
                        }
                    }
                    Err(error) => out.give(Err(error)).await,
                }
            },
        ))
    }

    /// Releases what the provider holds: Python's `close()`. Nothing by
    /// default.
    fn close(&self) -> impl Future<Output = ()> + Send {
        async {}
    }
}

/// The name of the type `T`, without its path nor its parameters.
fn short_type_name<T: ?Sized>() -> &'static str {
    let full = std::any::type_name::<T>();
    let base = full.split('<').next().unwrap_or(full);
    base.rsplit("::").next().unwrap_or(base)
}

#[cfg(test)]
mod tests;
