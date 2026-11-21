//! The provider against the wire cases captured on `RoomKit`'s Python
//! provider (`tests/fixtures/python_chat_cases.json`): each case whose call
//! is `generate` is replayed through a local fake of the endpoint, and the
//! requests the provider sends and the result it reads are compared with
//! Python's.
//!
//! No case is left out silently. A case the provider answers otherwise than
//! Python is named in [`QUESTIONED_OTHERWISE`] or in [`ONLY_PYTHON`], with
//! the reason and what the provider does instead, which is asserted; a named
//! case that starts to replay as Python fails the test.

pub(crate) mod fake;

use std::collections::BTreeMap;
use std::error::Error as _;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use roomkit_providers_core::{
    AiProvider, AiRequest, AiResponse, ProviderError, ResponseSchemaError,
};
use serde::Deserialize;
use serde_json::Value;

use self::fake::{Body, Ending, Fake, Reply, Seen};
use crate::provider::Pace;
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

/// What the provider does where it does not do as Python.
#[derive(Clone, Copy, Debug)]
enum Instead {
    /// It is not built: its failure says these words.
    Unbuilt(&'static str),
    /// It is built, where Python's is not.
    Built,
    /// The request fails, with this status, retried or not, in these words.
    Fails(Option<u16>, bool, &'static str),
    /// The request is answered, with this usage.
    Usage(&'static str),
}

const REDIRECT: &str = "A redirect is not followed: the maintainer's decision.";
const NO_REDIRECT: Instead = Instead::Fails(None, false, "redirect");
const COUNT: &str =
    "A count that is no JSON whole number stays unknown: it is neither converted nor fatal.";

/// The questioned cases the provider answers otherwise than Python: the
/// reason, and what it does instead.
const QUESTIONED_OTHERWISE: &[(&str, &str, Instead)] = &[
    (
        "base_url_with_a_trailing_slash_and_a_query",
        "A base URL with a query is refused when the provider is built: the maintainer's \
         decision. Python sends to a path written after the query.",
        Instead::Unbuilt("a query or a fragment"),
    ),
    ("redirect", REDIRECT, NO_REDIRECT),
    ("redirect_301", REDIRECT, NO_REDIRECT),
    ("redirect_302_to_another_host", REDIRECT, NO_REDIRECT),
    ("redirect_303", REDIRECT, NO_REDIRECT),
    ("redirect_308", REDIRECT, NO_REDIRECT),
    ("redirects_past_the_hop_limit", REDIRECT, NO_REDIRECT),
    (
        "usage_completion_tokens_as_text",
        COUNT,
        Instead::Usage(r#"{"input_tokens":12}"#),
    ),
    ("usage_counts_that_are_true", COUNT, Instead::Usage("{}")),
    (
        "usage_cached_tokens_as_text",
        COUNT,
        Instead::Usage(r#"{"input_tokens":1200,"output_tokens":45}"#),
    ),
    (
        "usage_reasoning_tokens_as_text",
        COUNT,
        Instead::Usage(r#"{"input_tokens":1200,"output_tokens":45}"#),
    ),
    (
        "usage_cache_write_written_as_text_in_the_details",
        COUNT,
        Instead::Usage(r#"{"input_tokens":1200,"output_tokens":45}"#),
    ),
    ("usage_counts_below_zero", COUNT, Instead::Usage("{}")),
];

/// The cases whose failure no provider reproduces: the openai SDK's own, or
/// a raw Python exception. What the provider does instead.
const ONLY_PYTHON: &[(&str, &str, Instead)] = &[
    (
        "empty_key",
        "The openai SDK builds no client without a key. The provider is built, as the \
         source's, and sends no Authorization.",
        Instead::Built,
    ),
    (
        "choices_that_are_no_list",
        "Python raises a raw AttributeError. The body is no completion.",
        Instead::Fails(None, false, "not a chat completion"),
    ),
    (
        "usage_prompt_tokens_as_text",
        "Python raises a raw TypeError. The count stays unknown.",
        Instead::Usage(r#"{"output_tokens":5}"#),
    ),
    (
        "usage_cached_tokens_as_a_word",
        "Python raises a raw TypeError. The count stays unknown.",
        Instead::Usage(r#"{"input_tokens":1200,"output_tokens":45}"#),
    ),
    (
        "usage_completion_tokens_as_a_word",
        "Python raises a raw ValidationError. The count stays unknown.",
        Instead::Usage(r#"{"input_tokens":12}"#),
    ),
    (
        "usage_counts_with_a_fraction",
        "Python raises a raw ValidationError. The counts stay unknown.",
        Instead::Usage("{}"),
    ),
    (
        "usage_reasoning_tokens_as_a_word",
        "Python raises a raw ValidationError. The count stays unknown.",
        Instead::Usage(r#"{"input_tokens":1200,"output_tokens":45}"#),
    ),
];

/// What replaying a case gave.
struct Replayed {
    /// The configuration of the case.
    config: OpenAiConfig,
    /// Why the provider was not built, when it was not.
    unbuilt: Option<ProviderError>,
    outcome: Option<Result<AiResponse, ProviderError>>,
    seen: Vec<Seen>,
    waits: Vec<f64>,
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
    let dialed = provider.endpoint().clone();
    let target = &dialed[url::Position::BeforePath..];
    provider.point_at(&format!("http://{}{target}", fake.address));
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
    replayed.outcome = Some(provider.generate(&request).await);
    replayed.seen = fake.seen();
    replayed.waits = waits.lock().unwrap().clone();
    replayed
}

/// Where the requests the provider sent differ from Python's.
fn requests_differ(case: &Case, replayed: &Replayed, found: &mut Vec<String>) {
    if replayed.seen.len() != case.exchanges.len() {
        found.push(format!(
            "{} requests sent, where Python sent {}",
            replayed.seen.len(),
            case.exchanges.len()
        ));
    }
    for (at, (seen, exchange)) in replayed.seen.iter().zip(&case.exchanges).enumerate() {
        let sent = &exchange.request;
        let url = reqwest::Url::parse(&sent.url).unwrap();
        let target = &url[url::Position::BeforePath..];
        if (seen.method.as_str(), seen.target.as_str()) != (sent.method.as_str(), target) {
            found.push(format!("request {at}: {} {}", seen.method, seen.target));
        }
        let body = serde_json::from_slice::<Value>(&seen.body).ok();
        let written = |body: Option<&Value>| body.map(Value::to_string);
        if written(body.as_ref()) != written(sent.body.as_ref()) {
            found.push(format!(
                "request {at}: its body is {:?}",
                written(body.as_ref())
            ));
        }
        for (name, value) in &sent.headers {
            let key = format!("Bearer {}", replayed.config.api_key);
            let expected = value
                .as_ref()
                .map(|value| value.replace("Bearer <config key>", &key));
            if seen.headers.get(name) != expected.as_ref() {
                found.push(format!(
                    "request {at}: {name} is {:?}",
                    seen.headers.get(name)
                ));
            }
        }
        let timeouts = [
            ("connect", replayed.config.connect_timeout),
            ("read", replayed.config.timeout),
        ];
        for (name, timeout) in timeouts {
            if sent.timeouts.get(name) != Some(&timeout.as_secs_f64()) {
                found.push(format!("request {at}: the {name} timeout is {timeout:?}"));
            }
        }
    }
}

/// Where the waits the provider took between its tries differ from the
/// SDK's: one read from the answer's headers is exact, a backoff is within
/// a quarter below what the fixture writes.
fn waits_differ(case: &Case, replayed: &Replayed, found: &mut Vec<String>) {
    let expected: Vec<&Wait> = case
        .exchanges
        .iter()
        .filter_map(|e| e.wait.as_ref())
        .collect();
    if expected.len() != replayed.waits.len() {
        found.push(format!(
            "{} waits, where Python took {}",
            replayed.waits.len(),
            expected.len()
        ));
    }
    for (taken, wait) in replayed.waits.iter().zip(expected) {
        let (least, most) = if wait.from == "the answer's headers" {
            (wait.seconds, wait.seconds)
        } else {
            (wait.seconds * 0.75, wait.seconds)
        };
        if *taken < least - 1e-6 || *taken > most + 1e-6 {
            found.push(format!(
                "a wait of {taken} s, where Python took {} s",
                wait.seconds
            ));
        }
    }
}

/// Where the response the provider read differs from Python's. An id Python
/// minted is random: its shape is compared, not its value.
fn response_differs(case: &Case, expected: &Value, response: &AiResponse, found: &mut Vec<String>) {
    let mut read = serde_json::to_value(response).unwrap();
    let calls = expected["tool_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate();
    for (at, call) in calls {
        let id = call["id"].as_str().unwrap_or_default();
        if !case.minted_call_ids.iter().any(|minted| minted == id) {
            continue;
        }
        let Some(given) = read["tool_calls"][at]["id"].as_str() else {
            continue;
        };
        let (name, drawn) = given.rsplit_once('_').unwrap_or_default();
        let shaped = drawn.len() == 12 && drawn.bytes().all(|byte| byte.is_ascii_hexdigit());
        if shaped
            && id
                .rsplit_once('_')
                .is_some_and(|(expected, _)| expected == name)
        {
            read["tool_calls"][at]["id"] = id.into();
        }
    }
    // As texts: the members in the same order too.
    let (read, expected) = (read.to_string(), expected.to_string());
    if read != expected {
        found.push(format!("the response is {read}"));
    }
}

/// Where the failure of the provider differs from Python's, by what the
/// case says a replay reproduces.
fn failure_differs(case: &Case, failure: &Failure, error: &ProviderError, found: &mut Vec<String>) {
    let Some(compare) = &case.compare else {
        return found.push("a failure with no rule to compare it".to_owned());
    };
    if compare.exactly.is_empty() {
        return found.push(format!("a {} no provider reproduces", failure.class));
    }
    let schema = error
        .source()
        .and_then(|source| source.downcast_ref::<ResponseSchemaError>());
    for attribute in &compare.exactly {
        let same = match attribute.as_str() {
            "class" => (failure.class == "ResponseSchemaError") == schema.is_some(),
            "status_code" => failure.status_code == error.status_code,
            "retryable" => failure.retryable == Some(error.retryable),
            "provider" => failure.provider.as_deref() == Some(error.provider.as_str()),
            "context_overflow" => failure.context_overflow == error.context_overflow,
            "before_request" => failure.before_request == Some(error.before_request),
            "reason" => failure.reason.as_deref() == schema.map(|schema| schema.reason.as_str()),
            other => panic!("{}: {other} is not compared", case.name),
        };
        if !same {
            found.push(format!("the failure's {attribute} differs: {error:?}"));
        }
    }
    let compared = matches!(compare.message.as_str(), "roomkit" | "server");
    if compared && error.message != failure.message {
        found.push(format!("the failure says {:?}", error.message));
    }
}

/// Every way `replayed` differs from what Python did of `case`; none when
/// the provider replays the case as Python.
fn differences(case: &Case, replayed: &Replayed) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(unbuilt) = &replayed.unbuilt {
        found.push(format!("the provider is not built: {}", unbuilt.message));
        return found;
    }
    requests_differ(case, replayed, &mut found);
    waits_differ(case, replayed, &mut found);
    match (
        &replayed.outcome,
        &case.expected.response,
        &case.expected.error,
    ) {
        (Some(Ok(response)), Some(expected), _) => {
            response_differs(case, expected, response, &mut found);
        }
        (Some(Err(error)), _, Some(failure)) => failure_differs(case, failure, error, &mut found),
        (Some(Ok(response)), None, _) => found.push(format!(
            "an answer where Python fails: {}",
            serde_json::to_string(response).unwrap()
        )),
        (Some(Err(error)), _, None) => {
            found.push(format!("a failure where Python answers: {error:?}"));
        }
        (None, _, _) => found.push("nothing was asked".to_owned()),
    }
    found
}

/// Whether `replayed` did what `instead` says.
fn does(instead: Instead, replayed: &Replayed) -> bool {
    match (instead, &replayed.unbuilt, &replayed.outcome) {
        (Instead::Unbuilt(words), Some(error), _) => {
            error.before_request && error.message.contains(words)
        }
        (Instead::Built, None, _) => true,
        (Instead::Fails(status, retryable, words), None, Some(Err(error))) => {
            (error.status_code == status || status.is_none())
                && error.retryable == retryable
                && error.message.contains(words)
        }
        (Instead::Usage(usage), None, Some(Ok(response))) => {
            serde_json::to_string(&response.usage).unwrap() == usage
        }
        _ => false,
    }
}

fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).unwrap()
}

/// The cases the provider answers otherwise than Python, by name.
fn named() -> BTreeMap<&'static str, Instead> {
    let named = QUESTIONED_OTHERWISE.iter().chain(ONLY_PYTHON);
    named.map(|(name, _, instead)| (*name, *instead)).collect()
}

/// Every `generate` case of the fixture: replayed as Python, or named with
/// what the provider does instead.
#[tokio::test]
async fn every_generate_case_replays_as_python_or_is_named() {
    let fixture = fixture();
    assert_eq!(fixture.clock, "2026-01-01T00:00:00Z");
    let named = named();
    let (mut replayed_as_python, mut failures) = (0, Vec::new());
    for case in fixture.cases.iter().filter(|case| case.call == "generate") {
        let replayed = replay(case).await;
        let differing = differences(case, &replayed);
        match named.get(case.name.as_str()) {
            None if differing.is_empty() => replayed_as_python += 1,
            None => failures.push(format!("{}: {}", case.name, differing.join("; "))),
            Some(_) if differing.is_empty() => {
                failures.push(format!("{}: named, and it replays as Python", case.name));
            }
            Some(instead) if !does(*instead, &replayed) => {
                let outcome = replayed.outcome.map(|outcome| outcome.map(|r| r.usage));
                let unbuilt = replayed.unbuilt;
                failures.push(format!(
                    "{}: not {instead:?}: {unbuilt:?} {outcome:?}",
                    case.name
                ));
            }
            Some(_) => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(replayed_as_python + named.len(), 203);
}

/// Each named case is a `generate` case of the fixture, questioned there or
/// failing with what no provider reproduces, and says why.
#[test]
fn each_named_case_is_one_the_fixture_sets_apart() {
    let fixture = fixture();
    let case = |name: &str| {
        let found = fixture.cases.iter().find(|case| case.name == name);
        found.unwrap_or_else(|| panic!("no case {name}"))
    };
    for (name, reason, _) in QUESTIONED_OTHERWISE {
        assert_eq!(case(name).call, "generate", "{name}");
        assert!(case(name).questioned.is_some(), "{name}");
        assert!(reason.ends_with('.'), "{name}");
    }
    for (name, reason, _) in ONLY_PYTHON {
        let compare = case(name).compare.as_ref();
        assert!(
            compare.is_some_and(|compare| compare.exactly.is_empty()),
            "{name}"
        );
        assert!(reason.ends_with('.'), "{name}");
    }
    assert_eq!(
        named().len(),
        QUESTIONED_OTHERWISE.len() + ONLY_PYTHON.len()
    );
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
