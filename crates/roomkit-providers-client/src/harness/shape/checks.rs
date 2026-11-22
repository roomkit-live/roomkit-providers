//! The checks a fixture is held to: what each case must say of itself for a
//! replay to rest on it.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use crate::harness::python_repr::repr;
use roomkit_ai::{
    AiContext, AiResponse, ModelInfo, ProviderError, ProviderErrorOptions, ResponseSchemaError,
    ResponseSchemaErrorOptions, ResponseSchemaFailure, StreamEvent,
};
use roomkit_providers_python_forms::{As, Form};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{Case, Chunk, Exchange, Failure, Failures, Fixture, Shape};

/// What Python writes in place of a key a request carries.
const MASK: &str = "**********";

/// The attributes of a `ProviderError` a replay reproduces as written; a
/// response schema's failure adds its `reason`.
const EXACT: [&str; 6] = [
    "class",
    "status_code",
    "retryable",
    "provider",
    "context_overflow",
    "before_request",
];

/// Whose words a failure's message may hold, and whether a replay compares
/// it: `RoomKit`'s own and the server's are, the others are not.
const WORDS: [(&str, bool); 7] = [
    ("roomkit", true),
    ("server", true),
    ("sdk", false),
    ("python_json", false),
    ("python_repr", false),
    ("transport", false),
    ("python", false),
];

/// The classes the HTTP client raises for a body it could not read to its
/// end.
const TRANSPORT_ERRORS: [&str; 4] = [
    "RemoteProtocolError",
    "ReadTimeout",
    "ReadError",
    "ConnectError",
];

/// The two kinds of case that pin a behaviour of Python that is not
/// settled, as their sentence starts.
const POSSIBLE_DEFECT: &str = "A possible defect: ";
const TO_DECIDE: &str = "A difference to decide: ";

/// How the reads of a body may end.
const ENDINGS: [&str; 3] = [
    "the stream ends",
    "the connection is lost",
    "the read times out",
];

/// The kind of a case whose reads cut inside characters.
const CUT_INSIDE_A_CHARACTER: &str = "stream_cut_inside_a_character";

/// Reads `value` as `T` and requires that `T` writes it back unchanged, its
/// members in the same order.
fn read_back<T: DeserializeOwned + Serialize>(value: &Value, label: &str) -> T {
    let read = T::deserialize(value).unwrap_or_else(|error| panic!("{label}: {error}"));
    let written = serde_json::to_string(&read).ok();
    assert_eq!(written, Some(value.to_string()), "{label}");
    read
}

/// Fails for a `ProviderError` of the fixture that lacks the field `name`.
fn missing<T>(label: &str, name: &str) -> T {
    panic!("{label}: a ProviderError without {name}")
}

/// The error a `ProviderError` of the contract reads as, with the fields
/// Python's carried.
fn provider_error(failure: &Failure, label: &str) -> ProviderError {
    let provider = failure.provider.clone();
    let provider = provider.unwrap_or_else(|| missing(label, "provider"));
    let before_request = failure.before_request;
    let before_request = before_request.unwrap_or_else(|| missing(label, "before_request"));
    let retryable = failure.retryable;
    let options = ProviderErrorOptions {
        retryable: retryable.unwrap_or_else(|| missing(label, "retryable")),
        provider: provider.clone(),
        status_code: failure.status_code,
        context_overflow: failure.context_overflow,
        before_request,
    };
    let Some(reason) = &failure.reason else {
        assert_eq!(failure.class, "ProviderError", "{label}");
        return ProviderError::new(failure.message.clone(), options);
    };
    assert_eq!(failure.class, "ResponseSchemaError", "{label}");
    let reasons = [
        ResponseSchemaFailure::Unsupported,
        ResponseSchemaFailure::Refusal,
        ResponseSchemaFailure::Truncated,
        ResponseSchemaFailure::InvalidJson,
    ];
    let reason = reasons.into_iter().find(|known| known.as_str() == reason);
    let reason = reason.unwrap_or_else(|| missing(label, "a reason the contract names"));
    let schema = ResponseSchemaErrorOptions { provider };
    let error: ProviderError =
        ResponseSchemaError::new(failure.message.clone(), reason, schema).into();
    // What the contract derives from a schema's failure is what Python's
    // error carried.
    assert_eq!(
        (
            error.retryable,
            error.before_request,
            error.status_code,
            error.context_overflow
        ),
        (options.retryable, before_request, None, None),
        "{label}"
    );
    error
}

/// Every text of `value`, itself included.
fn texts_in(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::String(text) => found.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| texts_in(item, found)),
        Value::Object(members) => members.values().for_each(|item| texts_in(item, found)),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// What the fake sent in the exchanges of a case, as a failure's message
/// may hold it.
#[derive(Default)]
struct Served {
    /// A text body, each text of a JSON body, and each text of the JSON a
    /// stream's `data` lines hold.
    texts: Vec<String>,
    /// The data of each `data` line of a stream, as it was written.
    data: Vec<String>,
    /// Python's `repr` of each JSON document a `data` line holds.
    reprs: Vec<String>,
}

impl Served {
    fn of(exchanges: &[Exchange]) -> Self {
        let mut served = Self::default();
        for answer in exchanges.iter().map(|exchange| &exchange.response) {
            let bodies = answer.text.iter().map(|text| text.trim().to_owned());
            served.texts.extend(bodies);
            let found = &mut served.texts;
            answer.json.iter().for_each(|body| texts_in(body, found));
            let reads = answer.chunks.iter().flatten().filter_map(Chunk::bytes);
            let sent: Vec<u8> = reads.flatten().collect();
            for line in String::from_utf8_lossy(&sent).lines() {
                let data = line.strip_prefix("data:");
                served.data.extend(data.map(|data| data.trim().to_owned()));
                if let Ok(event) = serde_json::from_str::<Value>(data.unwrap_or(line)) {
                    texts_in(&event, &mut served.texts);
                    served.reprs.extend(data.map(|_| repr(&event)));
                }
            }
        }
        served
    }
}

impl Shape<'_> {
    /// Whose words the message of `failure` holds, read from the failure
    /// itself and from what the fake `served`: the label the fixture must
    /// give it.
    fn words_of(&self, failure: &Failure, served: &Served) -> &'static str {
        let message = failure.message.as_str();
        let cause = failure.raised_from.as_deref();
        let said_by_the_sdk =
            self.sdk_errors.contains(&failure.class.as_str()) || self.sdk_words.contains(&message);
        if said_by_the_sdk {
            return "sdk";
        }
        if failure.provider.is_none() {
            return "python";
        }
        if cause.is_some_and(|class| self.sdk_causes.contains(&class)) {
            return "sdk";
        }
        if cause == Some("JSONDecodeError") {
            return "python_json";
        }
        if cause.is_some_and(|class| TRANSPORT_ERRORS.contains(&class)) {
            return "transport";
        }
        if let Some(body) = message.strip_prefix("Error code: ") {
            // Python's repr of the body follows the status, when a body came.
            return if body.contains(" - ") {
                "python_repr"
            } else {
                "sdk"
            };
        }
        let raised = |held: &[String]| {
            self.raises_an_event_s_data && held.iter().any(|text| text == message)
        };
        if raised(&served.reprs) {
            return "python_repr";
        }
        if served.texts.iter().any(|text| text == message) || raised(&served.data) {
            return "server";
        }
        if self
            .sdk_sentences
            .iter()
            .any(|start| message.starts_with(start))
        {
            return "sdk";
        }
        if matches!(cause, None | Some("ValueError")) {
            "roomkit"
        } else {
            "python"
        }
    }
}

impl Fixture {
    /// The fixture `text` writes, with no member of a case that `shape`
    /// does not name.
    #[must_use]
    pub fn read(text: &str, shape: &Shape<'_>) -> Self {
        let fixture: Self = serde_json::from_str(text).unwrap();
        for case in &fixture.cases {
            for member in case.own.keys() {
                let known = shape.own_members.contains(&member.as_str());
                assert!(known, "{}: an unknown member {member}", case.name);
            }
        }
        fixture
    }

    /// How many cases make `call`.
    #[must_use]
    pub fn calls(&self, call: &str) -> usize {
        let made = self.cases.iter().filter(|case| case.call == call);
        made.count()
    }

    /// The fixture says the versions it was written from, the commit and
    /// whether its files were clean, its clock, and how each part is
    /// written.
    pub fn says_where_it_comes_from(&self, shape: &Shape<'_>) {
        let sdk = self.others.get(shape.sdk_version).and_then(Value::as_str);
        let sdk = sdk.unwrap_or_else(|| panic!("no {}", shape.sdk_version));
        for version in [
            self.roomkit_version.as_str(),
            sdk,
            &self.pydantic_version,
            &self.httpx2_version,
            &self.python_version,
        ] {
            assert!(
                version.starts_with(|c: char| c.is_ascii_digit()),
                "{version}"
            );
        }
        if let Some(commit) = &self.roomkit_commit {
            let hexadecimal = commit.bytes().all(|byte| byte.is_ascii_hexdigit());
            assert!(commit.len() == 40 && hexadecimal, "{commit}");
        }
        // Whether the files were clean is said with the commit, and only
        // with it; a fixture captured from files with changes not committed
        // names a commit it was not written from.
        let clean = self.roomkit_commit.as_ref().map(|_| false);
        assert_eq!(self.roomkit_dirty, clean);
        assert_eq!(self.clock, "2026-01-01T00:00:00Z");
        for note in shape.notes {
            assert!(self.notes.contains_key(*note), "no note on {note}");
        }
        assert!(self.notes["environment"].contains(shape.environment));
        assert!(self.notes["exchanges.response"].contains("hex"));
    }

    /// Each request reads as the contract's request and writes back
    /// unchanged, but for the cases `shape` says the contract does not
    /// read. A key the request carries for itself is written masked, and
    /// plain beside it.
    pub fn each_request_reads_as_the_contract_s(&self, shape: &Shape<'_>) {
        let mut unread = Vec::new();
        for case in &self.cases {
            let name = &case.name;
            if AiContext::read(case.request.clone()).is_err() {
                unread.push(name.as_str());
                continue;
            }
            read_back::<As<AiContext>>(&case.request, name);
            let written = case.request["metadata"].get("api_key");
            assert_eq!(case.request_api_key.is_some(), written.is_some(), "{name}");
            let (Some(key), Some(written)) = (&case.request_api_key, written) else {
                continue;
            };
            // A key is a text that is not empty: anything else is written
            // as it is.
            match key.as_str().filter(|key| !key.is_empty()) {
                Some(key) => {
                    assert!(shape.request_keys.contains(&key), "{name}");
                    assert_eq!(written, MASK, "{name}");
                }
                None => assert_eq!(written, key, "{name}"),
            }
        }
        assert_eq!(unread, shape.unread);
    }

    /// Each result reads as the contract's types and writes back unchanged:
    /// the response, the models and the events; and a `ProviderError`
    /// reads as the contract's, with the text Python's had.
    pub fn each_result_reads_as_the_contract_s(&self, shape: &Shape<'_>) {
        for case in &self.cases {
            let (name, expected) = (&case.name, &case.expected);
            let given = [
                expected.response.is_some(),
                expected.text.is_some(),
                expected.events.is_some(),
                expected.models.is_some(),
            ];
            let failed = expected.error.is_some();
            match case.call.as_str() {
                "generate" => assert_eq!(given, [!failed, false, false, false], "{name}"),
                "generate_stream" => {
                    assert!(given == [false, true, false, false] || failed, "{name}");
                }
                "generate_structured_stream" => {
                    assert!(given == [false, false, true, false] || failed, "{name}");
                }
                "list_models" => assert_eq!(given, [false, false, false, !failed], "{name}"),
                other => panic!("{name}: an unknown call {other}"),
            }
            if let Some(response) = &expected.response {
                read_back::<As<AiResponse>>(response, name);
            }
            for model in expected.models.iter().flatten() {
                read_back::<ModelInfo>(model, name);
            }
            for event in expected.events.iter().flatten() {
                read_back::<As<StreamEvent>>(event, name);
            }
            let Some(failure) = &expected.error else {
                continue;
            };
            if failure.provider.is_none() {
                // No error of the contract: the SDK's own, or a raw Python
                // exception.
                assert_ne!(failure.class, "ProviderError", "{name}");
                assert_ne!(failure.class, "ResponseSchemaError", "{name}");
                continue;
            }
            let error = provider_error(failure, name);
            assert_eq!(error.to_string(), failure.text, "{name}");
            assert_eq!(error.provider, shape.provider, "{name}");
            if error.before_request {
                assert!(case.exchanges.is_empty(), "{name}");
            }
            assert!(
                failure.raised_from.as_deref() != Some(""),
                "{name}: an empty class"
            );
        }
    }

    /// Every failure says how a replay compares it, and no other case
    /// does: what it reproduces as written, the reason of a response
    /// schema's failure included, and whose words its message holds. How
    /// many failures the fixture holds, for a provider to require enough.
    #[must_use]
    pub fn each_failure_says_what_a_replay_reproduces(&self, shape: &Shape<'_>) -> Failures {
        let (mut compared, mut failures, mut reasons) = (0, 0, 0);
        for case in &self.cases {
            let (name, expected) = (&case.name, &case.expected);
            assert_eq!(case.compare.is_some(), expected.error.is_some(), "{name}");
            let (Some(compare), Some(failure)) = (&case.compare, &expected.error) else {
                continue;
            };
            failures += 1;
            // The label is the one the failure and the answers stand for.
            let words = shape.words_of(failure, &Served::of(&case.exchanges));
            assert_eq!(compare.message, words, "{name}");
            let is_compared = WORDS.iter().find(|(known, _)| *known == words);
            compared += usize::from(is_compared.is_some_and(|(_, compared)| *compared));
            if failure.provider.is_none() {
                // A failure no provider reproduces.
                assert!(compare.exactly.is_empty(), "{name}");
                continue;
            }
            let reason = failure.reason.as_ref().map(|_| "reason");
            reasons += usize::from(reason.is_some());
            let given = [
                ("events", expected.events.is_some()),
                ("text", expected.text.is_some()),
            ];
            let given = given.iter().filter(|(_, held)| *held).map(|(key, _)| *key);
            let exactly: Vec<&str> = EXACT.into_iter().chain(reason).chain(given).collect();
            assert_eq!(compare.exactly, exactly, "{name}");
            // A failure before any request holds RoomKit's words alone.
            if failure.before_request == Some(true) {
                assert_eq!(words, "roomkit", "{name}");
            }
        }
        Failures {
            all: failures,
            compared,
            of_a_schema: reasons,
        }
    }

    /// The cases that pin a behaviour of Python that is not settled are
    /// `questioned`, sorted by name, each with a sentence that starts with
    /// its kind; `to_decide` are those that claim no defect of Python's.
    pub fn the_questioned_cases_are(&self, questioned: &[&str], to_decide: &[&str]) {
        let marked = self.cases.iter().filter(|case| case.questioned.is_some());
        let mut names: Vec<&str> = marked.clone().map(|case| case.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, questioned);
        let mut listed: Vec<&str> = self.questioned.iter().map(String::as_str).collect();
        listed.sort_unstable();
        assert_eq!(listed, questioned);
        let mut deciding = Vec::new();
        for case in marked {
            let sentence = case.questioned.as_deref().unwrap_or_default();
            assert!(sentence.ends_with('.'), "{}", case.name);
            if sentence.starts_with(TO_DECIDE) {
                deciding.push(case.name.as_str());
            } else {
                assert!(sentence.starts_with(POSSIBLE_DEFECT), "{}", case.name);
            }
        }
        deciding.sort_unstable();
        assert_eq!(deciding, to_decide);
        assert!(self.notes["questioned"].contains(POSSIBLE_DEFECT.trim_end_matches(": ")));
        assert!(self.notes["questioned"].contains(TO_DECIDE.trim_end_matches(": ")));
    }

    /// Each request went where its case expects: to `first`, the URL the
    /// configuration of a case and its call give, or where the answer
    /// before it redirected it, a location read against the URL it
    /// answered. A case whose request is redirected to a location is
    /// questioned.
    pub fn each_request_was_sent_where_its_case_expects(&self, first: impl Fn(&Case) -> String) {
        for case in &self.cases {
            let name = &case.name;
            let mut expected = first(case);
            for exchange in &case.exchanges {
                assert_eq!(exchange.request.url, expected, "{name}");
                let answer = &exchange.response;
                let headers = answer.headers.as_ref();
                let location = headers.and_then(|headers| headers.get("location"));
                let redirected = matches!(answer.status, Some(300..=399));
                if let Some(location) = location.filter(|_| redirected) {
                    let from = reqwest::Url::parse(&expected).unwrap();
                    expected = from.join(location).unwrap().into();
                    assert!(case.questioned.is_some(), "{name}");
                }
            }
        }
    }

    /// The timeouts of a configuration are the ones each request was sent
    /// with: its own for a read, a write and the pool, the other to
    /// connect. `timeouts` gives the two of a case's configuration, the
    /// read's then the connection's, `None` for one that does not read.
    pub fn the_timeouts_are_the_ones_python_sent(
        &self,
        timeouts: impl Fn(&Value) -> Option<(Duration, Duration)>,
    ) {
        for case in &self.cases {
            let Some((whole, connect)) = timeouts(&case.config) else {
                continue;
            };
            for exchange in &case.exchanges {
                let sent: BTreeMap<&str, f64> = exchange
                    .request
                    .timeouts
                    .iter()
                    .map(|(name, seconds)| (name.as_str(), *seconds))
                    .collect();
                let expected = BTreeMap::from([
                    ("connect", connect.as_secs_f64()),
                    ("pool", whole.as_secs_f64()),
                    ("read", whole.as_secs_f64()),
                    ("write", whole.as_secs_f64()),
                ]);
                assert_eq!(sent, expected, "{}", case.name);
            }
        }
    }

    /// Each exchange holds a request Python sent and one answer: a body or
    /// a failure, the reads of a stream with how they end, and the wait
    /// before each next try.
    pub fn each_exchange_holds_a_request_and_one_answer(&self, shape: &Shape<'_>) {
        for case in &self.cases {
            let name = &case.name;
            for exchange in &case.exchanges {
                let sent = &exchange.request;
                // A listing, and a redirect the SDK follows as a GET, carry
                // no body; a listing says of none what it is.
                assert_eq!(sent.method == "POST", sent.body.is_some(), "{name}");
                if case.call == "list_models" {
                    assert_eq!(sent.method, "GET", "{name}");
                    assert_eq!(sent.headers.get("content-type"), Some(&None), "{name}");
                }
                assert!(["POST", "GET"].contains(&sent.method.as_str()), "{name}");
                for value in sent.headers.values().flatten() {
                    assert!(!value.contains(shape.key), "{name}: a key in a header");
                }
                let answer = &exchange.response;
                let bodies = [
                    answer.json.is_some(),
                    answer.text.is_some(),
                    answer.chunks.is_some(),
                    answer.failure.is_some(),
                ];
                assert_eq!(bodies.iter().filter(|held| **held).count(), 1, "{name}");
                assert_eq!(answer.status.is_some(), answer.failure.is_none(), "{name}");
                assert_eq!(answer.headers.is_some(), answer.failure.is_none(), "{name}");
                assert_eq!(answer.then.is_some(), answer.chunks.is_some(), "{name}");
                if let Some(then) = &answer.then {
                    assert!(ENDINGS.contains(&then.as_str()), "{name}: {then}");
                }
                if let Some(wait) = &exchange.wait {
                    let taken = wait.seconds > 0.0 && wait.seconds <= shape.longest_wait;
                    assert!(taken, "{name}");
                    assert!(wait.from.starts_with("the "), "{name}");
                }
            }
            // The SDK waits before each next try: after every exchange but
            // the last, and but a redirect, which it follows at once.
            let last = case.exchanges.len().saturating_sub(1);
            for (at, exchange) in case.exchanges.iter().enumerate() {
                let redirected = matches!(exchange.response.status, Some(300..=399));
                let waited = exchange.wait.is_some();
                assert_eq!(waited, at != last && !redirected, "{name}");
            }
        }
    }

    /// A chunk holds any bytes: a text is the UTF-8 it stands for, and
    /// bytes that are no UTF-8 are written in hexadecimal. Where reads cut
    /// inside characters, the reads together are the whole text. How many
    /// reads end inside a character.
    #[must_use]
    pub fn each_chunk_reads_as_the_bytes_of_one_read(&self) -> usize {
        let mut cut_inside = 0;
        for case in &self.cases {
            let name = &case.name;
            for exchange in &case.exchanges {
                let Some(chunks) = &exchange.response.chunks else {
                    continue;
                };
                let mut whole = Vec::new();
                let mut partial = 0;
                for chunk in chunks {
                    let bytes = chunk.bytes().unwrap_or_else(|| panic!("{name}: a chunk"));
                    if matches!(chunk, Chunk::Bytes { .. }) {
                        // Bytes that read as a text are written as that text.
                        assert!(std::str::from_utf8(&bytes).is_err(), "{name}");
                        partial += 1;
                    }
                    whole.extend(bytes);
                }
                let inside = case.kinds.iter().any(|kind| kind == CUT_INSIDE_A_CHARACTER);
                assert_eq!(partial > 0, inside, "{name}");
                if inside {
                    assert!(std::str::from_utf8(&whole).is_ok(), "{name}");
                    cut_inside += partial;
                }
            }
        }
        cut_inside
    }

    /// An id Python minted is random: a replay compares its shape. Each is
    /// in the result of its case; a case of the kind `minted`, when a
    /// fixture has one, holds some.
    pub fn each_minted_call_id_has_python_s_shape(&self, minted: Option<&str>) {
        for case in &self.cases {
            let expected = serde_json::to_string(&case.expected.response).unwrap()
                + &serde_json::to_string(&case.expected.events).unwrap();
            for id in &case.minted_call_ids {
                assert!(expected.contains(&format!("\"{id}\"")), "{}", case.name);
                let drawn = id.rsplit('_').next().unwrap_or_default();
                assert!(id.starts_with("call_"), "{}", case.name);
                assert!(
                    drawn.len() == 12 && drawn.bytes().all(|byte| byte.is_ascii_hexdigit()),
                    "{}",
                    case.name
                );
            }
            if case.kinds.iter().any(|kind| Some(kind.as_str()) == minted) {
                assert!(!case.minted_call_ids.is_empty(), "{}", case.name);
            }
        }
    }

    /// Where the SDK decides, the case says so: its retries, a redirect it
    /// follows, and a failure whose words are its own.
    pub fn a_case_the_sdk_decides_says_so(&self) {
        for case in &self.cases {
            let tried_again = case.exchanges.len() > 1;
            let its_words = case
                .compare
                .as_ref()
                .is_some_and(|compare| compare.message == "sdk");
            if tried_again || its_words {
                assert!(case.sdk.is_some(), "{}", case.name);
            }
        }
    }

    /// The fixture shows each of `kinds` in at least one case, no other
    /// kind, and no case twice.
    pub fn covers_every_kind_of_case(&self, kinds: &[&str]) {
        let known: BTreeSet<&str> = kinds.iter().copied().collect();
        assert_eq!(known.len(), kinds.len(), "a kind is listed twice");
        let mut seen = BTreeSet::new();
        let mut names = BTreeSet::new();
        for case in &self.cases {
            assert!(names.insert(case.name.as_str()), "{} twice", case.name);
            for kind in &case.kinds {
                assert!(known.contains(kind.as_str()), "{}: {kind}", case.name);
                seen.insert(kind.as_str());
            }
        }
        assert_eq!(seen, known);
    }
}
