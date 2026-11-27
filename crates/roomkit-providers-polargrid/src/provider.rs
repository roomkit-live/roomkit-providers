//! The PolarGrid provider: Python's `PolarGridAIProvider`, on the chat
//! completions of a PolarGrid edge.
//!
//! The edge is the pinned region's, or, where none is pinned, the one the
//! autorouter picks for the model, asked once, at the first call. A whole
//! answer and the listing are tried again only when they got no answer, as
//! polargrid-sdk tries them, after one second, then two, up to the
//! configuration's `max_retries`; a stream is tried once.

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use roomkit_ai::{
    AiContext, AiProvider, AiResponse, ModelInfo, ProviderError, ProviderStream, StreamDone,
    TextStream, checked_stream, listing, schema_for_generate,
};
use roomkit_providers_client::endpoint::joined;
use roomkit_providers_client::retry::{Backoff, Past, Rule};
use roomkit_providers_client::stream::{leaving, text, timed};
use roomkit_providers_client::{Call, Client, Exit, Settings, Timeouts, tried_again};
use serde::Deserialize;
use serde_json::Value;

use crate::answer;
use crate::catalogue::{self, PolarGridRegion};
use crate::config::PolarGridConfig;
use crate::failure::{self, Failure, past_mark};
use crate::request::Asked;
use crate::routing::{self, AUTOROUTER, Pick, Picks};
use crate::stream;

/// The provider its failures name.
pub(crate) const LABEL: &str = "polargrid";

/// The provider's name, as Python's class names itself.
const CLASS_NAME: &str = "PolarGridAIProvider";

/// The variable the SDK reads a key from when it is given none.
const KEY_VARIABLE: &str = "POLARGRID_API_KEY";

/// How long the autorouter is waited for, to connect and to answer.
const ROUTER_TIMEOUT: Duration = Duration::from_secs(2);

/// The SDK's waits between tries of a request that got no answer.
const BACKOFF: Backoff = Backoff { first: 1.0 };

/// The rule a server's wait is read by: no answer is tried again, so it
/// only bounds what one may ask.
const RETRY: Rule = Rule {
    longest_asked: 60.0,
    past_it: Past::Longest,
};

/// The edge a provider sends its requests to.
#[derive(Debug)]
pub(crate) struct Edge {
    pub(crate) chat: reqwest::Url,
    pub(crate) models: reqwest::Url,
    region_id: Option<String>,
    region_name: Option<String>,
}

impl Edge {
    fn of(pick: Pick) -> Result<Self, Failure> {
        Ok(Self {
            chat: joined(&pick.endpoint, "/v1/chat/completions")?,
            models: joined(&pick.endpoint, "/v1/models")?,
            region_id: pick.region_id,
            region_name: pick.region_name,
        })
    }
}

/// A model on a PolarGrid edge (RFC §6.7): Python's
/// `PolarGridAIProvider(config)`. A turn's thinking switch goes as
/// `enable_thinking`, qwen's reasoning read back from its `<think>` tags;
/// an answer is bounded by what the model's window leaves; a response
/// schema goes strict.
#[derive(Clone)]
pub struct PolarGrid {
    pub(crate) config: Arc<PolarGridConfig>,
    /// The client of a whole answer and of the listing, which tries again
    /// what got no answer.
    pub(crate) client: Client,
    /// The client of a stream, which tries nothing again.
    pub(crate) streaming: Client,
    /// The client that asks the autorouter.
    pub(crate) router_client: Client,
    pub(crate) router: reqwest::Url,
    pinned: Option<Arc<Edge>>,
    routed: Arc<Mutex<Option<Arc<Edge>>>>,
    picks: Arc<Picks>,
    headers: HeaderMap,
    pub(crate) streamed_headers: HeaderMap,
    /// Where a test or a replay sends every request, the edge the
    /// autorouter picks included.
    #[cfg(any(test, feature = "replay"))]
    fake: Option<std::net::SocketAddr>,
    exit: Arc<Exit>,
}

impl PolarGrid {
    /// The provider `config` configures. The environment is read then, and
    /// only then: for the proxies and the certificate authorities, as every
    /// provider reads them, and, for a configuration with an empty key, for
    /// `POLARGRID_API_KEY`, as the SDK reads it.
    ///
    /// # Errors
    ///
    /// A failure before any request: a key that cannot go in a header, a
    /// timeout the clock cannot count.
    pub fn new(config: PolarGridConfig) -> Result<Self, ProviderError> {
        let from_the_environment = std::env::var(KEY_VARIABLE).ok();
        Self::built(config, from_the_environment).map_err(|failure| Exit::unbuilt(LABEL, &failure))
    }

    fn built(
        config: PolarGridConfig,
        from_the_environment: Option<String>,
    ) -> Result<Self, Failure> {
        let key = if config.api_key.is_empty() {
            from_the_environment.unwrap_or_default()
        } else {
            config.api_key.clone()
        };
        let (headers, streamed_headers) = headers_of(&key)?;
        let settings = |timeouts, max_retries| Settings {
            timeouts,
            max_retries,
            retry: RETRY,
            refused: failure::status,
            tried_again: tried_again!("roomkit::providers::polargrid"),
        };
        let timeouts = Timeouts::new(config.timeout, config.connect_timeout)?;
        let mut client = Client::new(settings(timeouts, config.max_retries))?;
        client.try_again_only_unanswered(BACKOFF);
        let streaming = Client::new(settings(timeouts, 0))?;
        let router_client =
            Client::new(settings(Timeouts::new(ROUTER_TIMEOUT, ROUTER_TIMEOUT)?, 0))?;
        let router = reqwest::Url::parse(AUTOROUTER)
            .map_err(|_| Failure::unbuilt("its autorouter is no URL"))?;
        let pinned = config.region.as_deref().map(pinned_edge).transpose()?;
        let shown = pinned
            .as_ref()
            .map_or_else(|| router.clone(), |edge| edge.chat.clone());
        let exit = client.exit(LABEL, [key], [], &shown);
        Ok(Self {
            config: Arc::new(config),
            client,
            streaming,
            router_client,
            router,
            pinned,
            routed: Arc::default(),
            picks: Picks::shared(),
            headers,
            streamed_headers,
            #[cfg(any(test, feature = "replay"))]
            fake: None,
            exit: Arc::new(exit),
        })
    }

    /// The name the provider's failures carry: Python's `provider`.
    #[must_use]
    pub fn label(&self) -> &'static str {
        LABEL
    }

    /// The PolarGrid edges, as the catalogue lists them (Python's
    /// `available_regions`): PolarGrid serves no live list of them.
    #[must_use]
    pub fn available_regions() -> Vec<PolarGridRegion> {
        catalogue::regions().to_vec()
    }

    /// The edge the provider sends its requests to (Python's
    /// `connected_region`): the pinned one, or the one the autorouter picked,
    /// asked now if no call asked it yet; its location from the catalogue,
    /// and its name too where the edge has none, or an empty one.
    ///
    /// # Errors
    ///
    /// An edge the autorouter picked that is no URL.
    pub async fn connected_region(&self) -> Result<PolarGridRegion, ProviderError> {
        let edge = self
            .edge()
            .await
            .map_err(|failure| self.exit.leave(&failure))?;
        let known = edge.region_id.as_deref().and_then(catalogue::region);
        let named = edge.region_name.clone().filter(|name| !name.is_empty());
        Ok(PolarGridRegion {
            id: edge.region_id.clone(),
            name: named.or_else(|| known.and_then(|known| known.name.clone())),
            location: known.and_then(|known| known.location.clone()),
        })
    }

    pub(crate) fn asked(&self) -> Asked<'_> {
        Asked {
            config: &self.config,
            provider: LABEL,
        }
    }

    /// The edge: the pinned one, else the one the autorouter picks, asked
    /// once.
    pub(crate) async fn edge(&self) -> Result<Arc<Edge>, Failure> {
        if let Some(pinned) = &self.pinned {
            return Ok(Arc::clone(pinned));
        }
        if let Some(routed) = self.routed.lock().ok().and_then(|routed| routed.clone()) {
            return Ok(routed);
        }
        let pick = routing::pick(
            &self.router_client,
            &self.router,
            &self.config.model,
            &self.picks,
        )
        .await;
        let mut edge = Edge::of(pick)?;
        self.moved_to_the_fake(&mut edge)?;
        let edge = Arc::new(edge);
        if let Ok(mut routed) = self.routed.lock() {
            routed.get_or_insert_with(|| Arc::clone(&edge));
        }
        Ok(edge)
    }

    /// The edge the autorouter picked, sent to the fake a test or a replay
    /// points the provider at: through a copy of the client, which already
    /// sends there directly.
    #[cfg(any(test, feature = "replay"))]
    fn moved_to_the_fake(&self, edge: &mut Edge) -> Result<(), Failure> {
        let Some(fake) = self.fake else {
            return Ok(());
        };
        let mut mover = self.client.clone();
        if mover.pointed_at([&mut edge.chat, &mut edge.models], fake) {
            Ok(())
        } else {
            Err(Failure::unbuilt(
                "the edge picked cannot be sent to the fake",
            ))
        }
    }

    #[cfg(not(any(test, feature = "replay")))]
    #[expect(clippy::unused_self, reason = "the same call where no fake can be")]
    #[expect(
        clippy::unnecessary_wraps,
        reason = "the same call where no fake can be"
    )]
    fn moved_to_the_fake(&self, _edge: &mut Edge) -> Result<(), Failure> {
        Ok(())
    }

    /// Every request sent to `fake` instead, over plain HTTP, the edge the
    /// autorouter picks included, with a cache of its picks of its own.
    /// Each of the three clients sends there directly.
    #[cfg(any(test, feature = "replay"))]
    fn pointed_at_the_fake(&mut self, fake: std::net::SocketAddr) -> bool {
        let mut moved = self.router_client.pointed_at([&mut self.router], fake);
        moved &= self.streaming.pointed_at(std::iter::empty(), fake);
        moved &= self.client.pointed_at(std::iter::empty(), fake);
        if let Some(pinned) = &self.pinned {
            let mut edge = Edge {
                chat: pinned.chat.clone(),
                models: pinned.models.clone(),
                region_id: pinned.region_id.clone(),
                region_name: pinned.region_name.clone(),
            };
            moved &= self
                .client
                .pointed_at([&mut edge.chat, &mut edge.models], fake);
            self.pinned = Some(Arc::new(edge));
        }
        self.fake = Some(fake);
        self.picks = Arc::default();
        moved
    }

    /// Sends the requests to `address` instead, over plain HTTP: a test's
    /// fake of the endpoint.
    #[cfg(test)]
    pub(crate) fn point_at(&mut self, address: std::net::SocketAddr) {
        let moved = self.pointed_at_the_fake(address);
        assert!(moved, "the endpoint is pointed at the fake");
    }

    /// The provider a replay of recorded cases drives: its requests sent to
    /// `fake`, a loopback address, over plain HTTP, a date read against
    /// `clock`.
    ///
    /// # Errors
    ///
    /// A `fake` that is no loopback address.
    #[cfg(feature = "replay")]
    #[doc(hidden)]
    pub fn replaying(
        mut self,
        fake: std::net::SocketAddr,
        clock: f64,
    ) -> Result<(Self, ReplayWaits), ProviderError> {
        let mut check = self.router.clone();
        roomkit_providers_client::replayed_at([&mut check], fake)
            .map_err(|failure| self.exit.leave(&failure))?;
        self.pointed_at_the_fake(fake);
        let waits = self.client.replaying(clock);
        Ok((self, waits))
    }

    /// The answer to `request`, asked whole.
    async fn answer(&self, request: &AiContext) -> Result<AiResponse, Failure> {
        schema_for_generate(request, true, false, LABEL)?;
        let edge = self.edge().await?;
        let body = self.asked().body(request, false)?;
        let body = serde_json::to_vec(&body).map_err(|_| Failure::unwritten())?;
        let call = Call {
            url: &edge.chat,
            headers: &self.headers,
            body: Some(&body),
            token: None,
        };
        let (answer, _) = self.client.send(call).await?;
        answer::read(&answer, request, &self.config.model, LABEL)
    }

    /// The models the edge lists, each with the capabilities the catalogue
    /// knows of it.
    async fn live_models(&self) -> Result<Vec<ModelInfo>, Failure> {
        let edge = self.edge().await?;
        let call = Call {
            url: &edge.models,
            headers: &self.headers,
            body: None,
            token: None,
        };
        let (page, _) = self.client.send(call).await?;
        let page: Value =
            serde_json::from_slice(past_mark(&page)).map_err(|error| Failure::malformed(&error))?;
        let page = Listing::deserialize(&page).map_err(|error| failure::unread(&error))?;
        let live = page.data.into_iter().map(|model| {
            let mut read = ModelInfo::new(model.id.as_str());
            let known = catalogue::entry(&model.id);
            read.capabilities = known
                .map(|known| known.capabilities.clone())
                .unwrap_or_default();
            read
        });
        Ok(live.collect())
    }
}

/// The headers of a whole answer's request and of a stream's, with `key`:
/// the SDK's, the one it accepts back aside.
fn headers_of(key: &str) -> Result<(HeaderMap, HeaderMap), Failure> {
    let bearer = HeaderValue::from_str(&format!("Bearer {key}"));
    let mut bearer = bearer.map_err(|_| Failure::unbuilt("its key cannot go in a header"))?;
    bearer.set_sensitive(true);
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(AUTHORIZATION, bearer);
    let mut streamed_headers = headers.clone();
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    streamed_headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
    Ok((headers, streamed_headers))
}

/// The edge of the pinned `region`, as the SDK resolves it.
fn pinned_edge(region: &str) -> Result<Arc<Edge>, Failure> {
    let (endpoint, region_id, region_name) = catalogue::pinned(region);
    let pick = Pick {
        endpoint,
        region_id: Some(region_id),
        region_name,
    };
    Ok(Arc::new(Edge::of(pick)?))
}

/// A page of the listing, as the SDK reads it (`ListModelsResponse`).
#[derive(Deserialize)]
struct Listing {
    data: Vec<Listed>,
}

/// A model of the listing (`ModelInfo`): what the SDK requires of it.
#[derive(Deserialize)]
struct Listed {
    id: String,
    #[expect(dead_code, reason = "read for the SDK's check: a model is dated")]
    created: i64,
    #[expect(dead_code, reason = "read for the SDK's check: a model has an owner")]
    owned_by: String,
    #[expect(dead_code, reason = "read for the SDK's check: a model has a root")]
    root: String,
}

/// A refusal the done event carries: a constrained answer a content filter
/// stopped.
fn content_filtered(done: &StreamDone) -> Option<String> {
    let filtered = done.finish_reason == Some(roomkit_ai::FinishReason::ContentFilter);
    filtered.then(|| "content_filter".to_owned())
}

impl AiProvider for PolarGrid {
    fn name(&self) -> &'static str {
        CLASS_NAME
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    /// Whether the model sees images: its catalogue entry says it; an
    /// unknown model sees none.
    fn supports_vision(&self) -> bool {
        catalogue::entry(&self.config.model).and_then(|entry| entry.supports_vision) == Some(true)
    }

    fn available_models() -> Vec<ModelInfo> {
        catalogue::models().to_vec()
    }

    fn supports_response_schema(&self) -> bool {
        true
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let live = self.live_models().await;
        let live = live.map_err(|failure| self.exit.leave(&failure))?;
        Ok(listing(live, catalogue::curated()))
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_structured_streaming(&self) -> bool {
        true
    }

    async fn generate(&self, request: &AiContext) -> Result<AiResponse, ProviderError> {
        let answer = self.answer(request).await;
        answer.map_err(|failure| self.exit.leave_for(request, &failure))
    }

    fn generate_stream<'a>(&'a self, request: &'a AiContext) -> TextStream<'a> {
        text(self.generate_structured_stream(request))
    }

    fn generate_structured_stream<'a>(&'a self, request: &'a AiContext) -> ProviderStream<'a> {
        let events = timed(
            stream::events(self, request),
            self.name(),
            &self.config.model,
        );
        let checked = checked_stream(events, request, LABEL, content_filtered);
        leaving(checked, &self.exit, request)
    }
}

#[cfg(feature = "replay")]
#[doc(hidden)]
pub use roomkit_providers_client::ReplayWaits;

impl fmt::Debug for PolarGrid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PolarGrid")
            .field("config", &self.config)
            .field("timeouts", &self.client.timeouts())
            .finish_non_exhaustive()
    }
}

// The edge a provider reports, which a replay cannot see: the fake stands
// in for every host. Python: PolarGridAIProvider.connected_region.
#[cfg(test)]
mod tests {
    use roomkit_providers_client::harness::fake::{Fake, Reply};

    use super::*;

    async fn connected(region: Option<&str>, router: Vec<Reply>) -> PolarGridRegion {
        let fake = Fake::serving(router).await;
        let mut provider = PolarGrid::new(PolarGridConfig {
            region: region.map(str::to_owned),
            ..PolarGridConfig::new("placeholder-not-a-key")
        })
        .unwrap();
        provider.point_at(fake.address);
        provider.connected_region().await.unwrap()
    }

    fn region(id: &str, name: &str, location: Option<&str>) -> PolarGridRegion {
        PolarGridRegion {
            id: Some(id.to_owned()),
            name: Some(name.to_owned()),
            location: location.map(str::to_owned),
        }
    }

    #[tokio::test]
    async fn a_pinned_region_is_the_edge_its_name_or_its_id_resolves_to() {
        let toronto = region("yto-01", "Toronto", Some("Canada Central"));
        assert_eq!(connected(Some("Toronto"), Vec::new()).await, toronto);
        let west = region("sfo-01", "San Francisco", Some("US West"));
        assert_eq!(connected(Some("SFO"), Vec::new()).await, west);
    }

    #[tokio::test]
    async fn a_routed_region_is_the_pick_its_location_and_a_missing_name_from_the_catalogue() {
        let pick = r#"{"region": "yul-02", "endpoint": "https://api.yul-02.edge.polargrid.ai", "name": "Montreal 02"}"#;
        let picked = connected(None, vec![Reply::json(200, pick)]).await;
        assert_eq!(picked.name.as_deref(), Some("Montreal 02"));
        let unnamed = r#"{"region": "yto-01", "endpoint": "https://api.yto-01.edge.polargrid.ai", "name": ""}"#;
        let toronto = region("yto-01", "Toronto", Some("Canada Central"));
        assert_eq!(
            connected(None, vec![Reply::json(200, unnamed)]).await,
            toronto
        );
        let refused = Reply::json(404, r#"{"message": "no edge serves the model"}"#);
        assert_eq!(connected(None, vec![refused]).await, toronto);
    }
}
