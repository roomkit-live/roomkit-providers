//! The provider against the wire cases captured on `RoomKit`'s Python
//! provider (`tests/fixtures/python_chat_cases.json`): each case is
//! replayed through a local fake of the endpoint, which serves a stream
//! read by read, and what the provider sends and gives is compared with
//! Python's: the requests, the waits between two tries, and the response,
//! the events, the text or the models, a failure by the rule its case
//! gives. The fake says in how many reads it wrote each answer, which is
//! compared with the reads the case records.
//!
//! No case is left out silently. A case the provider answers otherwise than
//! Python is named in [`named`], with the reason, how its replay differs
//! from Python's and what the provider does instead: the replay must differ
//! in that and in nothing else, and the provider must do exactly that. A
//! named case that starts to replay as Python fails the test.

mod compare;
pub(crate) mod fake;
mod named;
mod self_test;

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use roomkit_providers_core::{
    AiProvider, AiRequest, AiResponse, ModelInfo, ProviderError, StreamEvent, next,
};
use serde::Deserialize;
use serde_json::Value;

use self::compare::differences;
use self::fake::{Body, Door, Ending, Fake, Reply, Seen};
use self::named::{Stated, named};
use crate::exchange::Pace;
use crate::provider::Timeouts;
use crate::{OpenAiAiProvider, OpenAiConfig};

const FIXTURE: &str = include_str!("../../tests/fixtures/python_chat_cases.json");

/// The fixture's clock, in seconds since 1970: 2026-01-01T00:00:00Z.
const CLOCK: f64 = 1_767_225_600.0;

/// How many times the replay lets the provider go on, after the fake went
/// silent, before it gives up on reading what the fake sent first.
const ROUNDS: usize = 4096;

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
    supports_vision: Option<bool>,
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
    /// The timeouts the provider's client was built with.
    timeouts: Option<Timeouts>,
    /// Whether the provider says its model takes images.
    vision: Option<bool>,
}

/// A stream of a provider's items.
type Items<'a, T> = Pin<Box<dyn futures_core::Stream<Item = Result<T, ProviderError>> + Send + 'a>>;

/// Every item of `stream`, and the failure that ended it.
pub(crate) async fn drained<T>(stream: Items<'_, T>) -> (Vec<T>, Option<ProviderError>) {
    counted(stream, &Mutex::new(0)).await
}

/// As [`drained`], counting in `given` the items as they come.
async fn counted<T>(
    mut stream: Items<'_, T>,
    given: &Mutex<usize>,
) -> (Vec<T>, Option<ProviderError>) {
    let mut items = Vec::new();
    while let Some(item) = next(&mut stream).await {
        match item {
            Ok(item) => items.push(item),
            Err(error) => return (items, Some(error)),
        }
        *given.lock().unwrap() = items.len();
    }
    (items, None)
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

/// The next step of `future`, without waiting for it.
pub(crate) async fn poll_once<F: Future + Unpin>(future: &mut F) -> Poll<F::Output> {
    std::future::poll_fn(|context| Poll::Ready(Pin::new(&mut *future).poll(context))).await
}

/// Lets `time` pass at once: the clock is stopped, moved and started again,
/// so that what waits on real answers afterwards waits in real time.
pub(crate) async fn let_pass(time: Duration) {
    tokio::time::pause();
    tokio::time::advance(time).await;
    tokio::time::resume();
}

/// Drives `call` to its end. Where the fake goes silent, the provider's
/// timeout is not waited for: once the provider read what the fake sent
/// before (`given` reaching `before` items, for a stream that gives some),
/// the time of `timeout` is let pass at once. Nothing here depends on how
/// fast anything arrives: a provider that never reads what was sent waits
/// for its timeout in real time, and the replay then says what differs.
async fn driven<T>(
    mut call: Pin<&mut (dyn Future<Output = T> + '_)>,
    fake: &Fake,
    given: &Mutex<usize>,
    before: usize,
    timeout: Duration,
) -> T {
    loop {
        tokio::select! {
            biased;
            ended = &mut call => return ended,
            () = fake.silent() => {}
        }
        let asked = fake.seen().len();
        for _ in 0..ROUNDS {
            // What was sent is read: the read that waits for more is the
            // one whose time is let pass.
            for _ in 0..8 {
                if let Poll::Ready(ended) = poll_once(&mut call).await {
                    return ended;
                }
                tokio::task::yield_now().await;
            }
            if fake.seen().len() != asked {
                // The next try began: its time is its own.
                break;
            }
            if *given.lock().unwrap() >= before {
                let_pass(timeout + Duration::from_millis(1)).await;
            }
        }
    }
}

/// Replays `case`: its provider built from its configuration, its endpoint
/// pointed at a fake behind `door` that answers with the case's exchanges,
/// its clock set to the fixture's and its waits kept instead of taken.
async fn replay(case: &Case, door: &Door) -> Replayed {
    replay_served(case, door, &reply_of).await
}

/// As [`replay`], the fake answering each exchange with what `reply` makes
/// of its answer.
async fn replay_served(
    case: &Case,
    door: &Door,
    reply: &(dyn Fn(&Answered) -> Reply + Sync),
) -> Replayed {
    let config: OpenAiConfig = serde_json::from_value(case.config.clone()).unwrap();
    let mut replayed = Replayed {
        config: config.clone(),
        unbuilt: None,
        outcome: None,
        seen: Vec::new(),
        waits: Vec::new(),
        timeouts: None,
        vision: None,
    };
    let mut provider = match OpenAiAiProvider::new(config) {
        Ok(provider) => provider,
        Err(error) => {
            replayed.unbuilt = Some(error);
            return replayed;
        }
    };
    replayed.timeouts = Some(provider.timeouts);
    replayed.vision = Some(provider.supports_vision());
    let replies = case.exchanges.iter().map(|e| reply(&e.response));
    let fake = Fake::behind(door, replies.collect());
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
    // What a stream gives before a silence, as Python read it.
    let expected = &case.expected;
    let before = expected.events.as_ref().map_or(0, Vec::len);
    let before = before.max(expected.text.as_ref().map_or(0, Vec::len));
    let given = Mutex::new(0);
    let timeout = provider.timeouts.read;
    let call = async {
        match case.call.as_str() {
            "generate" => Outcome::Response(provider.generate(&request).await),
            "generate_structured_stream" => {
                let events = provider.generate_structured_stream(&request);
                let (events, failure) = counted(events, &given).await;
                Outcome::Events(events, failure)
            }
            "generate_stream" => {
                let text = provider.generate_stream(&request);
                let (text, failure) = counted(text, &given).await;
                Outcome::Text(text, failure)
            }
            "list_models" => Outcome::Models(provider.list_models().await),
            other => panic!("{}: an unknown call {other}", case.name),
        }
    };
    let outcome = driven(std::pin::pin!(call), &fake, &given, before, timeout).await;
    replayed.outcome = Some(outcome);
    // The provider gone, every connection is closed, and the fake ends
    // serving each: what it saw then says how it wrote each answer.
    drop(provider);
    let served = tokio::time::timeout(Duration::from_secs(30), fake.served());
    served.await.expect("the fake still serves a connection");
    replayed.seen = fake.seen();
    replayed.waits = waits.lock().unwrap().clone();
    replayed
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).unwrap()
}

/// How a replayed case stands: `Ok(true)` as Python, `Ok(false)` as its
/// entry among the named cases states, else what is wrong: a difference
/// from Python of a case that is not named, a named case that replays as
/// Python, or one that does not differ or do as its entry states.
fn judged(case: &Case, stated: Option<&Stated>, replayed: &Replayed) -> Result<bool, String> {
    let differing = differences(case, replayed);
    match stated {
        None if differing.is_empty() => Ok(true),
        None => Err(differing.join("; ")),
        Some(_) if differing.is_empty() => Err("named, and it replays as Python".to_owned()),
        Some(stated) => stated.holds(&differing, replayed).map(|()| false),
    }
}

/// Replays every case of the fixture whose call is `call`: how many
/// replayed as Python, and how many are named and differ and do as their
/// entry states. Fails for a case that is neither, and for a named case
/// that replays as Python.
async fn replayed_or_named(call: &str) -> (usize, usize) {
    let fixture = fixture();
    assert_eq!(fixture.clock, "2026-01-01T00:00:00Z");
    let named = named();
    let (mut as_python, mut otherwise, mut failures) = (0, 0, Vec::new());
    // The cases are replayed one after the other, on one port.
    let door = Door::open().await;
    for case in fixture.cases.iter().filter(|case| case.call == call) {
        let replayed = replay(case, &door).await;
        match judged(case, named.get(case.name.as_str()), &replayed) {
            Ok(true) => as_python += 1,
            Ok(false) => otherwise += 1,
            Err(wrong) => failures.push(format!("{}: {wrong}", case.name)),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    (as_python, otherwise)
}

/// Every `generate` case of the fixture: replayed as Python, or named with
/// what the provider does instead.
#[tokio::test]
async fn every_generate_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named("generate").await, (186, 23));
}

/// Every `generate_structured_stream` case, the same way: the events in
/// their order, what was given before a failure, and the failure.
#[tokio::test]
async fn every_structured_stream_case_replays_as_python_or_is_named() {
    assert_eq!(
        replayed_or_named("generate_structured_stream").await,
        (98, 13)
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
    assert_eq!(replayed_or_named("list_models").await, (5, 5));
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
