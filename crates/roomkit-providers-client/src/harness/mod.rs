//! Test support for a provider crate: a provider replayed against the wire
//! cases captured on `RoomKit`'s Python provider.
//!
//! A [`Fixture`] holds the cases, each with a configuration, a request, the
//! HTTP exchanges Python had and what Python returned. Each case is
//! replayed through a local [`fake`] of the endpoint, which serves a stream
//! read by read, and what the provider sends and gives is compared with
//! Python's: the requests, the waits between two tries, and the response,
//! the events, the text or the models, a failure by the rule its case
//! gives. The fake says in how many reads it wrote each answer, which is
//! compared with the reads the case records.
//!
//! No case is left out silently. A case the provider answers otherwise than
//! Python is named ([`named`]), with the reason, how its replay differs
//! from Python's and what the provider does instead: the replay must differ
//! in that and in nothing else, and the provider must do exactly that. A
//! named case that starts to replay as Python fails the test.
//! [`replayed_or_named`] replays the cases of one call, and [`self_test`]
//! holds the replay to what it says it compares. [`shape`] holds a fixture
//! to its shape: every member of its cases read, strictly.
//!
//! A provider is driven through [`Replays`], which its crate implements in
//! its tests. Everything here fails a test by panicking: it is never for a
//! host.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_panics_doc,
    reason = "test support: a replay fails its test by panicking"
)]

mod compare;
pub mod fake;
#[cfg(test)]
mod least;
mod lent;
pub mod named;
mod python_float;
pub(crate) mod python_repr;
pub mod self_test;
pub mod shape;
pub mod tls;

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use crate::items::next;
use roomkit_ai::{
    API_KEY_METADATA_KEY, AiContext, AiProvider, AiResponse, ModelInfo, ProviderError, StreamEvent,
};
use roomkit_providers_python_forms::Form;
use serde::Deserialize;
use serde_json::Value;

use self::compare::differences;
use self::fake::{Body, Door, Ending, Fake, Reply, Seen};
use self::named::{Named, Stated};
use crate::{Client, Exit, Pace, Secrets, Timeouts};

/// How many times the replay lets the provider go on, after the fake went
/// silent, before it gives up on reading what the fake sent first.
const ROUNDS: usize = 4096;

/// A provider the replay drives: built from a case's configuration on a
/// [`Client`], whose clock the replay sets and whose waits it keeps.
pub trait Replays: AiProvider + Sized {
    /// The header that carries the provider's key, in lower case: what a
    /// provider built without a key does not send.
    const KEY_HEADER: &'static str;

    /// The provider of `config`, the configuration of a case as its fixture
    /// writes it.
    ///
    /// # Errors
    ///
    /// Why the provider is not built, when it is not.
    fn built(config: &Value) -> Result<Self, ProviderError>;

    /// The client the provider is built on.
    fn client(&mut self) -> &mut Client;

    /// Sends the provider's requests to `fake` instead, over plain HTTP,
    /// each to the path it had: what the provider knows of its endpoint
    /// stays what its configuration says.
    fn point_at(&mut self, fake: SocketAddr);

    /// The timeouts of the requests the provider sends through another
    /// client than [`client`](Self::client), by the start of the path they
    /// ask: none, unless the provider has such a client, as one that asks a
    /// server of its own where to send a request, with timeouts of its own.
    #[must_use]
    fn timeouts_by_path(&self) -> Vec<(&'static str, Timeouts)> {
        Vec::new()
    }

    /// What the provider's endpoint answers a request whose key it refuses:
    /// a 401 whose error object says `said` for its message, under `error`.
    /// A provider whose wire words a refusal otherwise writes its own.
    #[must_use]
    fn key_refused(said: &str) -> Reply {
        let error = serde_json::json!({"type": "authentication_error", "message": said});
        Reply::json(401, serde_json::json!({"error": error}).to_string())
    }
}

/// A fixture of wire cases, as the script that captured them on Python's
/// provider wrote it.
#[derive(Debug, Deserialize)]
pub struct Fixture {
    clock: String,
    /// The cases, in the order the fixture writes them.
    pub cases: Vec<Case>,
}

impl Fixture {
    /// The fixture `text` writes.
    #[must_use]
    pub fn read(text: &str) -> Self {
        serde_json::from_str(text).unwrap()
    }

    /// The fixture's clock, in seconds since 1970: the instant Python's
    /// clock was stopped at, which a `retry-after` date is read against.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "a date's seconds since the epoch are far within an f64's exact integers"
    )]
    pub fn clock(&self) -> f64 {
        let clock: jiff::Timestamp = self.clock.parse().unwrap();
        clock.as_second() as f64
    }

    /// The case named `name`.
    #[must_use]
    pub fn case(&self, name: &str) -> &Case {
        let found = self.cases.iter().find(|case| case.name == name);
        found.unwrap_or_else(|| panic!("no case {name}"))
    }
}

/// One wire case: what Python's provider was configured with and asked,
/// the exchanges it had, and what it returned.
#[derive(Debug, Deserialize)]
pub struct Case {
    /// The case's name, one of a kind in its fixture.
    pub name: String,
    /// The call of the provider the case makes: `generate`,
    /// `generate_structured_stream`, `generate_stream` or `list_models`.
    pub call: String,
    questioned: Option<String>,
    /// The configuration of the provider, as its fixture writes it.
    pub config: Value,
    supports_vision: Option<bool>,
    request: Value,
    /// What the request carries under its key for itself, which its JSON
    /// form masks when it is a key: a text that is not empty.
    request_api_key: Option<Value>,
    exchanges: Vec<Exchange>,
    expected: Expected,
    compare: Option<Compare>,
    #[serde(default)]
    minted_call_ids: Vec<String>,
    /// The members the replay does not read: a provider's own.
    #[serde(flatten)]
    others: serde_json::Map<String, Value>,
}

impl Case {
    /// A member of the case the replay does not read, which is a
    /// provider's own to check: what Python's provider read of its model's
    /// catalogue entry, a capability it answers.
    #[must_use]
    pub fn member(&self, name: &str) -> Option<&Value> {
        self.others.get(name)
    }

    /// The request of the case, with the key it carries for itself where
    /// its JSON form holds a mask.
    #[must_use]
    pub fn asked(&self) -> AiContext {
        self.read_request().unwrap()
    }

    /// As [`asked`](Self::asked), or why the contract does not read the
    /// request: it is stricter than Python's on what a request holds.
    fn read_request(&self) -> serde_json::Result<AiContext> {
        let mut request = AiContext::read(self.request.clone())?;
        if let Some(key) = &self.request_api_key {
            let key = key.clone();
            request
                .metadata
                .insert(API_KEY_METADATA_KEY.to_owned(), key);
        }
        Ok(request)
    }

    /// The key the case's request carries for itself, when it carries one.
    fn request_key(&self) -> Option<String> {
        let key = self.request_api_key.as_ref().and_then(Value::as_str);
        key.filter(|key| !key.is_empty()).map(str::to_owned)
    }

    /// What a fake answers each exchange of the case with, in order.
    #[must_use]
    pub fn replies(&self) -> Vec<Reply> {
        let answered = self.exchanges.iter().map(|exchange| &exchange.response);
        answered.map(reply_of).collect()
    }

    /// The response Python returned, when the case's call gives one.
    #[must_use]
    pub fn expected_response(&self) -> Option<&Value> {
        self.expected.response.as_ref()
    }

    /// The key of the case's configuration.
    fn key(&self) -> String {
        let key = self.config.get("api_key").and_then(Value::as_str);
        key.unwrap_or_default().to_owned()
    }
}

#[derive(Debug, Deserialize)]
struct Exchange {
    request: Sent,
    response: Answered,
    wait: Option<Wait>,
}

#[derive(Debug, Deserialize)]
struct Sent {
    method: String,
    url: String,
    headers: BTreeMap<String, Option<String>>,
    timeouts: BTreeMap<String, f64>,
    body: Option<Value>,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
struct Wait {
    seconds: f64,
    from: String,
}

#[derive(Debug, Deserialize)]
struct Expected {
    response: Option<Value>,
    events: Option<Vec<Value>>,
    text: Option<Vec<String>>,
    models: Option<Vec<Value>>,
    error: Option<Failure>,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
struct Compare {
    exactly: Vec<String>,
    message: String,
}

/// What the call of a case gave.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "one outcome a replayed case, held for a test: an answer is the large one"
)]
pub enum Outcome {
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
pub struct Replayed {
    /// The key of the case's configuration.
    key: String,
    /// The key the case's request carries for itself.
    request_key: Option<String>,
    /// Why the contract did not read the case's request, when it did not:
    /// nothing was asked then.
    pub unread: Option<String>,
    /// Why the provider was not built, when it was not.
    pub unbuilt: Option<ProviderError>,
    /// What the call gave, once the provider was built.
    pub outcome: Option<Outcome>,
    /// The requests the fake received, in order.
    pub seen: Vec<Seen>,
    /// The waits the provider asked for between its tries, in seconds:
    /// kept, not taken.
    pub waits: Vec<f64>,
    /// The timeouts the provider's client was built with.
    pub timeouts: Option<Timeouts>,
    /// The timeouts of the requests it sends through another client, by
    /// the start of their path.
    pub timeouts_by_path: Vec<(&'static str, Timeouts)>,
    /// Whether the provider says its model takes images.
    pub vision: Option<bool>,
}

impl fmt::Debug for Replayed {
    /// What the replay gave: neither of its keys.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Replayed")
            .field("unread", &self.unread)
            .field("unbuilt", &self.unbuilt)
            .field("outcome", &self.outcome)
            .field("seen", &self.seen)
            .field("waits", &self.waits)
            .field("timeouts", &self.timeouts)
            .field("vision", &self.vision)
            .finish_non_exhaustive()
    }
}

/// The exit of a test that has no client to make one: its failures carry
/// `provider` as their label and repeat none of `keys`. A provider gets its
/// own from its client ([`Client::exit`]).
#[must_use]
pub fn exit(provider: &'static str, keys: impl IntoIterator<Item = String>) -> Exit {
    Exit::new(provider, Secrets::of(keys))
}

/// A stream of a provider's items.
type Items<'a, T> = Pin<Box<dyn futures_core::Stream<Item = Result<T, ProviderError>> + Send + 'a>>;

/// Every item of `stream`, and the failure that ended it.
pub async fn drained<T>(stream: Items<'_, T>) -> (Vec<T>, Option<ProviderError>) {
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

/// The text the deltas of `events` wrote.
#[must_use]
pub fn text_of(events: &[StreamEvent]) -> String {
    let said = events.iter().filter_map(|event| match event {
        StreamEvent::TextDelta(delta) => Some(delta.text.as_str()),
        StreamEvent::ToolCall(_)
        | StreamEvent::ToolCallDelta(_)
        | StreamEvent::Done(_)
        | StreamEvent::ThinkingDelta(_)
        | StreamEvent::Usage(_) => None,
    });
    said.collect()
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
pub async fn poll_once<F: Future + Unpin>(future: &mut F) -> Poll<F::Output> {
    std::future::poll_fn(|context| Poll::Ready(Pin::new(&mut *future).poll(context))).await
}

/// Lets `time` pass at once: the clock is stopped, moved and started again,
/// so that what waits on real answers afterwards waits in real time.
pub async fn let_pass(time: Duration) {
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

/// Replays `case`, a case of `fixture`: its provider built from its
/// configuration, its endpoint pointed at a fake behind `door` that answers
/// with the case's exchanges, its clock set to the fixture's and its waits
/// kept instead of taken.
pub async fn replay<P: Replays>(fixture: &Fixture, case: &Case, door: &Door) -> Replayed {
    replay_served::<P>(fixture, case, door, &reply_of).await
}

/// As [`replay`], the fake answering each exchange with what `reply` makes
/// of its answer.
async fn replay_served<P: Replays>(
    fixture: &Fixture,
    case: &Case,
    door: &Door,
    reply: &(dyn Fn(&Answered) -> Reply + Sync),
) -> Replayed {
    let mut replayed = Replayed {
        key: case.key(),
        request_key: case.request_key(),
        unread: None,
        unbuilt: None,
        outcome: None,
        seen: Vec::new(),
        waits: Vec::new(),
        timeouts: None,
        timeouts_by_path: Vec::new(),
        vision: None,
    };
    let request = match case.read_request() {
        Ok(request) => request,
        Err(error) => {
            replayed.unread = Some(error.to_string());
            return replayed;
        }
    };
    let mut provider = match P::built(&case.config) {
        Ok(provider) => provider,
        Err(error) => {
            replayed.unbuilt = Some(error);
            return replayed;
        }
    };
    let timeouts = provider.client().timeouts();
    replayed.timeouts = Some(timeouts);
    replayed.timeouts_by_path = provider.timeouts_by_path();
    replayed.vision = Some(provider.supports_vision());
    let replies = case.exchanges.iter().map(|e| reply(&e.response));
    let fake = Fake::behind(door, replies.collect());
    provider.point_at(fake.address);
    let waits = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&waits);
    let clock = fixture.clock();
    provider.client().set_pace(Pace {
        now: Arc::new(move || clock),
        wait: Arc::new(move |wait| {
            kept.lock().unwrap().push(wait.as_secs_f64());
            Box::pin(std::future::ready(()))
        }),
    });
    // What a stream gives before a silence, as Python read it.
    let expected = &case.expected;
    let before = expected.events.as_ref().map_or(0, Vec::len);
    let before = before.max(expected.text.as_ref().map_or(0, Vec::len));
    let given = Mutex::new(0);
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
    let outcome = driven(std::pin::pin!(call), &fake, &given, before, timeouts.read()).await;
    replayed.outcome = Some(outcome);
    // The provider gone, every connection is closed, and the fake ends
    // serving each: what it saw then says how it wrote each answer.
    drop(provider);
    let served = tokio::time::timeout(Duration::from_secs(30), fake.served());
    served.await.expect("the fake still serves a connection");
    replayed.seen = fake.seen();
    replayed.waits.clone_from(&waits.lock().unwrap());
    replayed
}

/// How a replayed case stands: `Ok(true)` as Python, `Ok(false)` as its
/// entry among the named cases states, else what is wrong: a difference
/// from Python of a case that is not named, a named case that replays as
/// Python, or one that does not differ or do as its entry states.
fn judged<P: Replays>(
    case: &Case,
    stated: Option<&Stated>,
    replayed: &Replayed,
) -> Result<bool, String> {
    let differing = differences(case, replayed);
    match stated {
        None if differing.is_empty() => Ok(true),
        None => Err(differing.join("; ")),
        Some(_) if differing.is_empty() => Err("named, and it replays as Python".to_owned()),
        Some(stated) => stated.holds::<P>(&differing, replayed).map(|()| false),
    }
}

/// Replays every case of `fixture` whose call is `call`: how many replayed
/// as Python, and how many are among `named` and differ and do as their
/// entry states. Fails for a case that is neither, and for a named case
/// that replays as Python.
///
/// Each case of a generation is then asked once more, its request lending
/// a key that a 401 repeats: fails for a failure that holds that key, and
/// for a call none of whose cases shows the key kept out.
pub async fn replayed_or_named<P: Replays>(
    fixture: &Fixture,
    named: &Named,
    call: &str,
) -> (usize, usize) {
    let named = named.stated();
    let (mut as_python, mut otherwise, mut failures) = (0, 0, Vec::new());
    let mut kept_out = 0;
    // The cases are replayed one after the other, on one port.
    let door = Door::open().await;
    for case in fixture.cases.iter().filter(|case| case.call == call) {
        let replayed = replay::<P>(fixture, case, &door).await;
        match judged::<P>(case, named.get(case.name.as_str()), &replayed) {
            Ok(true) => as_python += 1,
            Ok(false) => otherwise += 1,
            Err(wrong) => failures.push(format!("{}: {wrong}", case.name)),
        }
        match lent::kept_out::<P>(case, &door).await {
            Ok(shown) => kept_out += usize::from(shown),
            Err(wrong) => failures.push(format!("{}: {wrong}", case.name)),
        }
    }
    // A listing is asked with the configuration's key: no request lends one.
    if call != "list_models" && kept_out == 0 {
        failures.push(format!(
            "{call}: no case shows the key lent to a request kept out of a failure"
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    (as_python, otherwise)
}

#[cfg(test)]
mod tests;
