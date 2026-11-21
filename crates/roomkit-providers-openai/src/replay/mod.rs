//! The provider against the wire cases captured on `RoomKit`'s Python
//! provider (`tests/fixtures/python_chat_cases.json`): each case is
//! replayed through a local fake of the endpoint, which serves a stream
//! read by read, and what the provider sends and gives is compared with
//! Python's: the requests, the waits between two tries, and the response,
//! the events, the text or the models, a failure by the rule its case
//! gives.
//!
//! No case is left out silently. A case the provider answers otherwise than
//! Python is named in [`named`], with the reason and what the provider does
//! instead, which is asserted; a named case that starts to replay as Python
//! fails the test.

mod compare;
pub(crate) mod fake;
mod named;

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use roomkit_providers_core::{
    AiProvider, AiRequest, AiResponse, ModelInfo, ProviderError, StreamEvent, next,
};
use serde::Deserialize;
use serde_json::Value;

use self::compare::differences;
use self::fake::{Body, Ending, Fake, Reply, Seen};
use self::named::named;
use crate::exchange::Pace;
use crate::{OpenAiAiProvider, OpenAiConfig};

const FIXTURE: &str = include_str!("../../tests/fixtures/python_chat_cases.json");

/// The fixture's clock, in seconds since 1970: 2026-01-01T00:00:00Z.
const CLOCK: f64 = 1_767_225_600.0;

/// The timeout a case that waits for one is replayed with, in place of its
/// configuration's: the wait is the same, only shorter.
const SHORT: Duration = Duration::from_millis(250);

#[derive(Deserialize)]
struct Fixture {
    clock: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    call: String,
    questioned: Option<String>,
    config: Value,
    catalogue: Option<Catalogue>,
    request: Value,
    exchanges: Vec<Exchange>,
    expected: Expected,
    compare: Option<Compare>,
    #[serde(default)]
    minted_call_ids: Vec<String>,
}

/// What Python's provider read of its model's catalogue entry.
#[derive(Deserialize)]
struct Catalogue {
    model: String,
    known: bool,
    tags: Vec<String>,
    reasoning_floor: Option<String>,
}

#[derive(Deserialize)]
struct Exchange {
    request: Sent,
    response: Answered,
    wait: Option<Wait>,
}

#[derive(Deserialize)]
struct Sent {
    method: String,
    url: String,
    headers: BTreeMap<String, Option<String>>,
    timeouts: BTreeMap<String, f64>,
    body: Option<Value>,
}

#[derive(Deserialize)]
struct Answered {
    status: Option<u16>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    json: Option<Value>,
    text: Option<String>,
    chunks: Option<Vec<Chunk>>,
    then: Option<String>,
    failure: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Chunk {
    Text(String),
    Bytes { hex: String },
}

impl Chunk {
    fn bytes(&self) -> Vec<u8> {
        match self {
            Self::Text(text) => text.clone().into_bytes(),
            Self::Bytes { hex } => hex
                .as_bytes()
                .chunks(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect(),
        }
    }
}

#[derive(Deserialize)]
struct Wait {
    seconds: f64,
    from: String,
}

#[derive(Deserialize)]
struct Expected {
    response: Option<Value>,
    events: Option<Vec<Value>>,
    text: Option<Vec<String>>,
    models: Option<Vec<Value>>,
    error: Option<Failure>,
}

#[derive(Deserialize)]
struct Failure {
    class: String,
    message: String,
    provider: Option<String>,
    retryable: Option<bool>,
    status_code: Option<u16>,
    context_overflow: Option<bool>,
    before_request: Option<bool>,
    reason: Option<String>,
}

#[derive(Deserialize)]
struct Compare {
    exactly: Vec<String>,
    message: String,
}

/// What the call of a case gave.
#[derive(Debug)]
enum Outcome {
    /// `generate`: the response, or the failure.
    Response(Result<AiResponse, ProviderError>),
    /// `generate_structured_stream`: the events, then the failure that
    /// ended them.
    Events(Vec<StreamEvent>, Option<ProviderError>),
    /// `generate_stream`: the text, then the failure that ended it.
    Text(Vec<String>, Option<ProviderError>),
    /// `list_models`: the models, or the failure.
    Models(Result<Vec<ModelInfo>, ProviderError>),
}

/// What replaying a case gave.
struct Replayed {
    /// The configuration of the case.
    config: OpenAiConfig,
    /// Why the provider was not built, when it was not.
    unbuilt: Option<ProviderError>,
    outcome: Option<Outcome>,
    seen: Vec<Seen>,
    waits: Vec<f64>,
}

/// Every item of `stream`, and the failure that ended it.
pub(crate) async fn drained<T>(
    mut stream: Pin<Box<dyn futures_core::Stream<Item = Result<T, ProviderError>> + Send + '_>>,
) -> (Vec<T>, Option<ProviderError>) {
    let mut given = Vec::new();
    while let Some(item) = next(&mut stream).await {
        match item {
            Ok(item) => given.push(item),
            Err(error) => return (given, Some(error)),
        }
    }
    (given, None)
}

fn reply_of(answered: &Answered) -> Reply {
    match answered.failure.as_deref() {
        Some("connection refused") => return Reply::Refused,
        Some(_) => return Reply::Silent,
        None => {}
    }
    let body = if let Some(chunks) = &answered.chunks {
        let ending = match answered.then.as_deref() {
            Some("the stream ends") => Ending::Ends,
            Some("the connection is lost") => Ending::Lost,
            _ => Ending::Silent,
        };
        Body::Reads(chunks.iter().map(Chunk::bytes).collect(), ending)
    } else if let Some(text) = &answered.text {
        Body::Whole(text.clone().into_bytes())
    } else {
        Body::Whole(serde_json::to_vec(&answered.json).unwrap())
    };
    Reply::Answer {
        status: answered.status.unwrap(),
        headers: answered.headers.clone().into_iter().collect(),
        body,
    }
}

/// Whether a case waits for a timeout: its provider is built with a short one.
fn waits_for_a_timeout(case: &Case) -> bool {
    case.exchanges.iter().any(|exchange| {
        let answered = &exchange.response;
        answered.failure.as_deref() == Some("timeout")
            || answered.then.as_deref() == Some("the read times out")
    })
}

/// Replays `case`: its provider built from its configuration, its endpoint
/// pointed at a fake that answers with the case's exchanges, its clock set
/// to the fixture's and its waits kept instead of taken.
async fn replay(case: &Case) -> Replayed {
    let config: OpenAiConfig = serde_json::from_value(case.config.clone()).unwrap();
    let mut built = config.clone();
    if waits_for_a_timeout(case) {
        built.timeout = SHORT;
    }
    let mut replayed = Replayed {
        config,
        unbuilt: None,
        outcome: None,
        seen: Vec::new(),
        waits: Vec::new(),
    };
    let mut provider = match OpenAiAiProvider::new(built) {
        Ok(provider) => provider,
        Err(error) => {
            replayed.unbuilt = Some(error);
            return replayed;
        }
    };
    let fake = Fake::serving(
        case.exchanges
            .iter()
            .map(|e| reply_of(&e.response))
            .collect(),
    )
    .await;
    provider.point_at(fake.address);
    let waits = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&waits);
    provider.pace = Pace {
        now: Arc::new(|| CLOCK),
        wait: Arc::new(move |wait| {
            kept.lock().unwrap().push(wait.as_secs_f64());
            Box::pin(std::future::ready(()))
        }),
    };
    let request: AiRequest = serde_json::from_value(case.request.clone()).unwrap();
    replayed.outcome = Some(match case.call.as_str() {
        "generate" => Outcome::Response(provider.generate(&request).await),
        "generate_structured_stream" => {
            let (events, failure) = drained(provider.generate_structured_stream(&request)).await;
            Outcome::Events(events, failure)
        }
        "generate_stream" => {
            let (text, failure) = drained(provider.generate_stream(&request)).await;
            Outcome::Text(text, failure)
        }
        "list_models" => Outcome::Models(provider.list_models().await),
        other => panic!("{}: an unknown call {other}", case.name),
    });
    replayed.seen = fake.seen();
    replayed.waits = waits.lock().unwrap().clone();
    replayed
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).unwrap()
}

/// Replays every case of the fixture whose call is `call`: how many
/// replayed as Python, and how many are named and did what their name says.
/// Fails for a case that is neither, and for a named case that replays as
/// Python.
async fn replayed_or_named(call: &str) -> (usize, usize) {
    let fixture = fixture();
    assert_eq!(fixture.clock, "2026-01-01T00:00:00Z");
    let named = named();
    let (mut as_python, mut otherwise, mut failures) = (0, 0, Vec::new());
    for case in fixture.cases.iter().filter(|case| case.call == call) {
        let replayed = replay(case).await;
        let differing = differences(case, &replayed);
        match named.get(case.name.as_str()) {
            None if differing.is_empty() => as_python += 1,
            None => failures.push(format!("{}: {}", case.name, differing.join("; "))),
            Some(_) if differing.is_empty() => {
                failures.push(format!("{}: named, and it replays as Python", case.name));
            }
            Some(instead) if !instead.is_done(&replayed) => {
                let (unbuilt, outcome) = (replayed.unbuilt, replayed.outcome);
                failures.push(format!(
                    "{}: not {instead:?}: {unbuilt:?} {outcome:?}",
                    case.name
                ));
            }
            Some(_) => otherwise += 1,
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    (as_python, otherwise)
}

/// Every `generate` case of the fixture: replayed as Python, or named with
/// what the provider does instead.
#[tokio::test]
async fn every_generate_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named("generate").await, (181, 22));
}

/// Every `generate_structured_stream` case, the same way: the events in
/// their order, what was given before a failure, and the failure.
#[tokio::test]
async fn every_structured_stream_case_replays_as_python_or_is_named() {
    assert_eq!(
        replayed_or_named("generate_structured_stream").await,
        (97, 13)
    );
}

/// Every `generate_stream` case: the text as it came.
#[tokio::test]
async fn every_text_stream_case_replays_as_python() {
    assert_eq!(replayed_or_named("generate_stream").await, (4, 0));
}

/// Every `list_models` case: the models, or what the provider does of a
/// failure Python's `list_models` raises as its SDK's or as a raw exception.
#[tokio::test]
async fn every_listing_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named("list_models").await, (4, 5));
}

/// The catalogue of the crate says of each model what the cases rest on:
/// a case that changed because Python's catalogue did shows here.
#[test]
fn the_catalogue_says_what_the_cases_rest_on() {
    let read = [
        crate::models::RESPONSES_ONLY,
        crate::models::CHAT_TOOLS_REFUSED,
        crate::models::TOOLS_REASONING_NONE,
        crate::models::TOOLS_REASONING_EFFORT,
    ];
    let mut models = 0;
    for case in fixture().cases {
        let Some(facts) = case.catalogue else {
            continue;
        };
        let entry = crate::models::entry(&facts.model);
        let tags = read
            .into_iter()
            .filter(|tag| crate::models::tagged(entry, tag));
        assert_eq!(entry.is_some(), facts.known, "{}", case.name);
        assert_eq!(tags.collect::<Vec<_>>(), facts.tags, "{}", case.name);
        let floor = crate::models::reasoning_floor(entry);
        assert_eq!(floor, facts.reasoning_floor.as_deref(), "{}", case.name);
        models += 1;
    }
    assert!(models >= 150, "{models} cases on OpenAI's own endpoint");
}
