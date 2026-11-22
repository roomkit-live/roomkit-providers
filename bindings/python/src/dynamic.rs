//! A provider held as an object, whichever its type: what Python is given.
//!
//! `RoomKit`'s core has such an object for a host that picks among
//! providers (`roomkit_ai::AnyProvider`), with the methods a channel
//! calls. Python calls more of a provider: a whole answer, a text stream,
//! the model listing and what it says it supports. This one has them all.

use std::future::Future;
use std::pin::Pin;

use roomkit_ai::{AiContext, AiProvider, AiResponse, ModelInfo, ProviderError, ProviderStream};

/// What a provider's call gives once awaited, held as an object.
type Held<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// An [`AiProvider`] as an object, which every provider is: what a host
/// that picks among providers of several types holds, as Python holds an
/// `AIProvider`. A caller never implements it: `Arc::new(provider)` is one.
///
/// Its methods are [`AiProvider`]'s under names of their own, so that a
/// provider with both traits in scope reads each method as one.
pub(crate) trait AnyProvider: Send + Sync + 'static {
    /// [`AiProvider::name`].
    fn dyn_name(&self) -> &str;

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
        request: &'a AiContext,
    ) -> Held<'a, Result<AiResponse, ProviderError>>;

    /// [`AiProvider::generate_structured_stream`].
    fn dyn_generate_structured_stream<'a>(&'a self, request: &'a AiContext) -> ProviderStream<'a>;

    /// [`AiProvider::close`].
    fn dyn_close(&self) -> Held<'_, ()>;
}

impl<P: AiProvider> AnyProvider for P {
    fn dyn_name(&self) -> &str {
        self.name()
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
        request: &'a AiContext,
    ) -> Held<'a, Result<AiResponse, ProviderError>> {
        Box::pin(self.generate(request))
    }

    fn dyn_generate_structured_stream<'a>(&'a self, request: &'a AiContext) -> ProviderStream<'a> {
        self.generate_structured_stream(request)
    }

    fn dyn_close(&self) -> Held<'_, ()> {
        Box::pin(self.close())
    }
}
