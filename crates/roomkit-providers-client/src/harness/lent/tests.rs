//! The check of a lent key held to what it says: a provider that leaves a
//! generation with its request passes it, and one that leaves without it,
//! or whose failure shows nothing of the refusal, does not.

use std::net::SocketAddr;

use reqwest::header::HeaderMap;
use roomkit_providers_llm::{
    AiProvider, AiRequest, AiResponse, ProviderError, ProviderStream, TextStream, Yielder, items,
};
use serde_json::{Value, json};

use super::kept_out;
use crate::endpoint::moved_to;
use crate::failure::{Stated, carried, past_mark};
use crate::harness::fake::Door;
use crate::harness::named::Named;
use crate::harness::{Fixture, Replays, replayed_or_named};
use crate::proxy::Proxies;
use crate::stream::{Item, leaving, text};
use crate::testing::{no_authority, settings, usual};
use crate::{Call, Client, Exit, Failure, Secrets};

/// The key of a test's provider: a placeholder.
const KEY: &str = "placeholder-not-a-key";

/// How the provider of a test leaves a generation, as its configuration
/// says under `leaves`.
const WITH_ITS_REQUEST: &str = "with its request";
const WITHOUT_ITS_REQUEST: &str = "without its request";
const SAYING_NOTHING: &str = "saying nothing of the refusal";
const ASKING_NOTHING: &str = "asking nothing";

/// A provider of the least wire: it posts an empty object, and gives the
/// failure of the answer.
struct Least {
    url: reqwest::Url,
    client: Client,
    exit: Exit,
    /// How it leaves a generation.
    leaves: String,
    /// A request that brings no key: what a provider that leaves without
    /// its request leaves with.
    no_request: AiRequest,
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
    fn left_with<'a>(&'a self, request: &'a AiRequest) -> &'a AiRequest {
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

    async fn generate(&self, request: &AiRequest) -> Result<AiResponse, ProviderError> {
        let asked = self.asked().await;
        let left = |failure| self.exit.leave_for(self.left_with(request), &failure);
        asked.map(|()| AiResponse::new("")).map_err(left)
    }

    fn generate_stream<'a>(&'a self, request: &'a AiRequest) -> TextStream<'a> {
        text(self.generate_structured_stream(request))
    }

    fn generate_structured_stream<'a>(&'a self, request: &'a AiRequest) -> ProviderStream<'a> {
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
            url: reqwest::Url::parse("https://llm.example/v1/answers").unwrap(),
            client: Client::built(stating, Proxies::default(), no_authority()).unwrap(),
            exit: Exit::new("least", Secrets::of([KEY.to_owned()])),
            leaves,
            no_request: AiRequest::default(),
        })
    }

    fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    fn point_at(&mut self, fake: SocketAddr) {
        assert!(moved_to(&mut self.url, fake));
    }
}

/// A fixture of one case for each generation, whose provider leaves as
/// `leaves` says.
fn fixture(leaves: &str) -> Fixture {
    let request = serde_json::to_value(AiRequest::default()).unwrap();
    let case = |call: &str| {
        json!({
            "name": call,
            "call": call,
            "config": {"api_key": KEY, "leaves": leaves},
            "request": request,
            "exchanges": [],
            "expected": {},
        })
    };
    let calls = ["generate", "generate_structured_stream", "generate_stream"];
    let cases: Vec<Value> = calls.into_iter().map(case).collect();
    Fixture::read(&json!({"clock": "2026-01-01T00:00:00Z", "cases": cases}).to_string())
}

/// What the check says of each generation of a provider that leaves as
/// `leaves` says.
async fn checked(leaves: &str) -> Vec<Result<bool, String>> {
    let (fixture, door) = (fixture(leaves), Door::open().await);
    let mut said = Vec::new();
    for case in &fixture.cases {
        said.push(kept_out::<Least>(case, &door).await);
    }
    said
}

// Each of the three generations is held: its failure says what the 401
// says, without the key lent to the request.
#[tokio::test]
async fn a_generation_that_leaves_with_its_request_keeps_the_lent_key_out() {
    assert_eq!(checked(WITH_ITS_REQUEST).await, vec![Ok(true); 3]);
}

// A provider that leaves a generation without its request repeats the key
// the 401 repeats: each of the three generations is refused.
#[tokio::test]
async fn a_generation_that_leaves_without_its_request_is_refused() {
    for wrong in checked(WITHOUT_ITS_REQUEST).await {
        let wrong = wrong.unwrap_err();
        let said = "the failure repeats the key lent to the request: ";
        assert!(wrong.starts_with(said), "{wrong}");
    }
}

// A failure that says nothing of the 401 shows nothing of the key: it is
// refused, where a request that was never sent is only not counted.
#[tokio::test]
async fn a_failure_that_shows_nothing_of_the_refusal_holds_nothing() {
    for wrong in checked(SAYING_NOTHING).await {
        let wrong = wrong.unwrap_err();
        let said = "the failure says nothing of the 401 that repeats the key lent to the request";
        assert!(wrong.starts_with(said), "{wrong}");
    }
    assert_eq!(checked(ASKING_NOTHING).await, vec![Ok(false); 3]);
}

// The replay of a fixture asks the check of each case: a provider that
// leaves without its request fails it there, for that reason among others.
#[tokio::test]
#[should_panic(expected = "the failure repeats the key lent to the request")]
async fn the_replay_of_a_fixture_holds_its_provider_to_the_check() {
    let named = Named {
        questioned_otherwise: &[],
        only_python: &[],
        by_its_own_rule: &[],
    };
    let fixture = fixture(WITHOUT_ITS_REQUEST);
    replayed_or_named::<Least>(&fixture, &named, "generate").await;
}
