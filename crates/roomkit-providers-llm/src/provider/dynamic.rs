//! A provider held as an object (RFC §6.7): Python takes any `AIProvider`
//! where it takes a provider, which a Rust trait with `async` methods
//! cannot be.

use std::future::Future;
use std::pin::Pin;

use super::{AiProvider, ProviderStream, TextStream};
use crate::catalog::ModelInfo;
use crate::error::ProviderError;
use crate::request::AiRequest;
use crate::response::AiResponse;

/// What a provider's call gives once awaited, held as an object.
type Held<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// An [`AiProvider`] as an object, which every provider is: what a host
/// that picks among providers of several types holds, as Python holds an
/// `AIProvider`. A caller never implements it: `Arc::new(provider)` is one.
///
/// Its methods are [`AiProvider`]'s under names of their own, so that a
/// provider with both traits in scope reads each method as one.
pub trait DynAiProvider: Send + Sync + 'static {
    /// [`AiProvider::name`].
    fn dyn_name(&self) -> &str;

    /// [`AiProvider::label`].
    fn dyn_label(&self) -> &str;

    /// [`AiProvider::model_name`].
    fn dyn_model_name(&self) -> &str;

    /// [`AiProvider::catalog_entry`].
    fn dyn_catalog_entry(&self) -> Option<ModelInfo>;

    /// [`AiProvider::supports_vision`].
    fn dyn_supports_vision(&self) -> bool;

    /// [`AiProvider::context_window`].
    fn dyn_context_window(&self) -> Option<u64>;

    /// [`AiProvider::supports_deferred_tools`].
    fn dyn_supports_deferred_tools(&self) -> bool;

    /// [`AiProvider::supports_streaming`].
    fn dyn_supports_streaming(&self) -> bool;

    /// [`AiProvider::supports_structured_streaming`].
    fn dyn_supports_structured_streaming(&self) -> bool;

    /// [`AiProvider::supports_response_schema`].
    fn dyn_supports_response_schema(&self) -> bool;

    /// [`AiProvider::supports_response_schema_with_tools`].
    fn dyn_supports_response_schema_with_tools(&self) -> bool;

    /// [`AiProvider::list_models`].
    fn dyn_list_models(&self) -> Held<'_, Result<Vec<ModelInfo>, ProviderError>>;

    /// [`AiProvider::generate`].
    fn dyn_generate<'a>(
        &'a self,
        request: &'a AiRequest,
    ) -> Held<'a, Result<AiResponse, ProviderError>>;

    /// [`AiProvider::generate_stream`].
    fn dyn_generate_stream<'a>(&'a self, request: &'a AiRequest) -> TextStream<'a>;

    /// [`AiProvider::generate_structured_stream`].
    fn dyn_generate_structured_stream<'a>(&'a self, request: &'a AiRequest) -> ProviderStream<'a>;

    /// [`AiProvider::close`].
    fn dyn_close(&self) -> Held<'_, ()>;

    /// The provider's type name, without its path nor its parameters
    /// (Python's `type(provider).__name__`).
    #[doc(hidden)]
    fn dyn_type_name(&self) -> &'static str;
}

impl<P: AiProvider> DynAiProvider for P {
    fn dyn_name(&self) -> &str {
        self.name()
    }

    fn dyn_label(&self) -> &str {
        self.label()
    }

    fn dyn_model_name(&self) -> &str {
        self.model_name()
    }

    fn dyn_catalog_entry(&self) -> Option<ModelInfo> {
        self.catalog_entry()
    }

    fn dyn_supports_vision(&self) -> bool {
        self.supports_vision()
    }

    fn dyn_context_window(&self) -> Option<u64> {
        self.context_window()
    }

    fn dyn_supports_deferred_tools(&self) -> bool {
        self.supports_deferred_tools()
    }

    fn dyn_supports_streaming(&self) -> bool {
        self.supports_streaming()
    }

    fn dyn_supports_structured_streaming(&self) -> bool {
        self.supports_structured_streaming()
    }

    fn dyn_supports_response_schema(&self) -> bool {
        self.supports_response_schema()
    }

    fn dyn_supports_response_schema_with_tools(&self) -> bool {
        self.supports_response_schema_with_tools()
    }

    fn dyn_list_models(&self) -> Held<'_, Result<Vec<ModelInfo>, ProviderError>> {
        Box::pin(self.list_models())
    }

    fn dyn_generate<'a>(
        &'a self,
        request: &'a AiRequest,
    ) -> Held<'a, Result<AiResponse, ProviderError>> {
        Box::pin(self.generate(request))
    }

    fn dyn_generate_stream<'a>(&'a self, request: &'a AiRequest) -> TextStream<'a> {
        self.generate_stream(request)
    }

    fn dyn_generate_structured_stream<'a>(&'a self, request: &'a AiRequest) -> ProviderStream<'a> {
        self.generate_structured_stream(request)
    }

    fn dyn_close(&self) -> Held<'_, ()> {
        Box::pin(self.close())
    }

    fn dyn_type_name(&self) -> &'static str {
        super::short_type_name::<P>()
    }
}
