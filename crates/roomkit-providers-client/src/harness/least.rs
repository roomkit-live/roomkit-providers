//! A provider of the least wire, for the harness's own tests: it posts an
//! empty object, and gives an empty answer or the failure of what it was
//! answered. Its configuration says how it leaves a generation.

use std::net::SocketAddr;

use crate::items::{Yielder, items};
use reqwest::header::HeaderMap;
use roomkit_ai::{AiContext, AiProvider, AiResponse, ProviderError, ProviderStream, TextStream};
use serde_json::Value;

use super::Replays;
use crate::endpoint::moved_to;
use crate::failure::{Stated, carried, past_mark};
use crate::proxy::Proxies;
use crate::stream::{Item, leaving, text};
use crate::testing::{no_authority, settings, usual};
use crate::{Call, Client, Exit, Failure, Secrets};

/// The key of a test's provider: a placeholder.
pub(crate) const KEY: &str = "placeholder-not-a-key";

/// The URL the provider asks.
pub(crate) const URL: &str = "https://llm.example/v1/answers";

/// How the provider of a test leaves a generation, as its configuration
/// says under `leaves`.
pub(crate) const WITH_ITS_REQUEST: &str = "with its request";
pub(crate) const WITHOUT_ITS_REQUEST: &str = "without its request";
pub(crate) const SAYING_NOTHING: &str = "saying nothing of the refusal";
pub(crate) const ASKING_NOTHING: &str = "asking nothing";

/// A provider of the least wire: it posts an empty object, and gives the
/// failure of the answer.
pub(crate) struct Least {
    url: reqwest::Url,
    client: Client,
    exit: Exit,
    /// How it leaves a generation.
    leaves: String,
    /// A request that brings no key: what a provider that leaves without
    /// its request leaves with.
    no_request: AiContext,
}

/// The failure of a refusal, in what its error object says for a message.
fn stated(status: u16, body: &[u8]) -> Failure {
    let parsed: Option<Value> = serde_json::from_slice(past_mark(body)).ok();
    let message = parsed
        .as_ref()
        .and_then(|body| body["error"]["message"].as_str());
    let stated = Stated {
        message: message.map(str::to_owned),
        ..Stated::default()
    };
    Failure::refused(status, body, stated)
}

impl Least {
    /// The failure of its one request, when it fails.
    async fn asked(&self) -> Result<(), Failure> {
        if self.leaves == ASKING_NOTHING {
            return Err(Failure::before_request("the request is refused"));
        }
        let headers = HeaderMap::new();
        let call = Call {
            url: &self.url,
            headers: &headers,
            body: Some(b"{}"),
        };
        self.client.send(call).await.map(|_| ())
    }

    /// The request a generation that answers `request` leaves with.
    fn left_with<'a>(&'a self, request: &'a AiContext) -> &'a AiContext {
        if self.leaves == WITHOUT_ITS_REQUEST {
            &self.no_request
        } else {
            request
        }
    }
}

impl AiProvider for Least {
    fn model_name(&self) -> &'static str {
        "least"
    }

    async fn generate(&self, request: &AiContext) -> Result<AiResponse, ProviderError> {
        let asked = self.asked().await;
        let left = |failure| self.exit.leave_for(self.left_with(request), &failure);
        asked.map(|()| AiResponse::new("")).map_err(left)
    }

    fn generate_stream<'a>(&'a self, request: &'a AiContext) -> TextStream<'a> {
        text(self.generate_structured_stream(request))
    }

    fn generate_structured_stream<'a>(&'a self, request: &'a AiContext) -> ProviderStream<'a> {
        let events = Box::pin(items(move |out: Yielder<Item>| async move {
            if let Err(failure) = self.asked().await {
                out.give(Err(carried(failure))).await;
            }
        }));
        leaving(events, &self.exit, self.left_with(request))
    }
}

impl Replays for Least {
    const KEY_HEADER: &'static str = "authorization";

    fn built(config: &Value) -> Result<Self, ProviderError> {
        let leaves = config["leaves"].as_str().unwrap().to_owned();
        let mut stating = settings(usual(), 0);
        if leaves != SAYING_NOTHING {
            stating.refused = stated;
        }
        Ok(Self {
            url: reqwest::Url::parse(URL).unwrap(),
            client: Client::built(stating, Proxies::default(), no_authority()).unwrap(),
            exit: Exit::new("least", Secrets::of([KEY.to_owned()])),
            leaves,
            no_request: AiContext::default(),
        })
    }

    fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    fn point_at(&mut self, fake: SocketAddr) {
        assert!(moved_to(&mut self.url, fake));
    }
}
