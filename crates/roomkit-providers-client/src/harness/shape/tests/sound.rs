//! A small fixture every check accepts, of a vendor that is none: one case
//! at least for each thing a check reads of a fixture.

use roomkit_ai::{
    AiContext, AiResponse, AiToolCall, ModelInfo, ProviderError, ProviderErrorOptions,
    ResponseSchemaError, ResponseSchemaErrorOptions, ResponseSchemaFailure, StreamDone,
    StreamEvent, StreamTextDelta,
};
use roomkit_providers_python_forms::Form;
use serde_json::{Value, json};

/// The label of the fixture's provider.
pub(super) const PROVIDER: &str = "vendor";

/// The key of every configuration: a placeholder.
pub(super) const KEY: &str = "placeholder-not-a-key";

/// The key a request carries for itself: a placeholder.
pub(super) const LENT_KEY: &str = "placeholder-lent-key";

/// The base URL of every configuration.
const BASE_URL: &str = "https://llm.example/v1";

/// What a replay reproduces as written of a `ProviderError`.
const EXACT: [&str; 6] = [
    "class",
    "status_code",
    "retryable",
    "provider",
    "context_overflow",
    "before_request",
];

/// The id Python minted for the call of the case `minted`.
pub(super) const MINTED: &str = "call_lookup_0123456789ab";

/// A request Python sent: a `POST` of an empty object to `path`.
fn posted(path: &str) -> Value {
    json!({
        "method": "POST",
        "url": format!("{BASE_URL}{path}"),
        "headers": {"authorization": "Bearer <config key>", "content-type": "application/json"},
        "timeouts": {"connect": 5.0, "pool": 60.0, "read": 60.0, "write": 60.0},
        "body": {},
    })
}

/// A request Python sent: a `GET` of `url`.
fn got(url: &str) -> Value {
    json!({
        "method": "GET",
        "url": url,
        "headers": {"authorization": "Bearer <config key>", "content-type": null},
        "timeouts": {"connect": 5.0, "pool": 60.0, "read": 60.0, "write": 60.0},
        "body": null,
    })
}

/// An answer of `status` whose body is `body`, as JSON.
fn answer(status: u16, body: &Value) -> Value {
    json!({"status": status, "headers": {"content-type": "application/json"}, "json": body})
}

/// An answer read in `reads`, which end as `then` says.
fn reads(reads: &Value, then: &str) -> Value {
    let headers = json!({"content-type": "text/event-stream"});
    json!({"status": 200, "headers": headers, "chunks": reads, "then": then})
}

/// One exchange with no wait after it.
fn exchange(request: Value, response: Value) -> Value {
    let members = [("request", request), ("response", response)];
    let members = members.map(|(name, held)| (name.to_owned(), held));
    Value::Object(members.into_iter().collect())
}

/// The one exchange of a case that asks once and is answered.
fn asked_once() -> Value {
    json!([exchange(
        posted("/answers"),
        answer(200, &json!({"said": "Yes."}))
    )])
}

/// A case named `name` of the kind `kind`, which makes `call` and has
/// `exchanges`, Python giving `expected`.
fn case(name: &str, kind: &str, call: &str, exchanges: &Value, expected: &Value) -> Value {
    json!({
        "name": name,
        "kinds": [kind],
        "call": call,
        "config": {"api_key": KEY, "base_url": BASE_URL, "timeout": 60.0, "connect_timeout": 5.0},
        "request": AiContext::default().written(),
        "exchanges": exchanges,
        "expected": expected,
    })
}

/// What Python gave of a case that answers `content`.
fn response(content: &str) -> Value {
    json!({"response": AiResponse::new(content).written()})
}

/// A `generate` case named `name` of the kind `kind` that asks once and
/// answers.
fn answered(name: &str, kind: &str) -> Value {
    case(name, kind, "generate", &asked_once(), &response("Yes."))
}

/// A `ProviderError` Python raised, as the fixture writes it: what it
/// says, its status, whether a new request may succeed and whether it
/// came before any request. Its text is the contract's.
pub(super) fn provider_error(
    provider: &str,
    message: &str,
    status: Option<u16>,
    retryable: bool,
    before_request: bool,
) -> Value {
    let options = ProviderErrorOptions {
        retryable,
        provider: provider.to_owned(),
        status_code: status,
        context_overflow: None,
        before_request,
    };
    json!({
        "class": "ProviderError",
        "message": message,
        "text": ProviderError::new(message, options).to_string(),
        "retryable": retryable,
        "provider": provider,
        "status_code": status,
        "context_overflow": null,
        "before_request": before_request,
    })
}

/// The failure of a response schema Python raised, as the fixture writes
/// it, with what the contract derives of it.
fn schema_error(message: &str, reason: ResponseSchemaFailure) -> Value {
    let options = ResponseSchemaErrorOptions {
        provider: PROVIDER.to_owned(),
    };
    let error: ProviderError = ResponseSchemaError::new(message, reason, options).into();
    json!({
        "class": "ResponseSchemaError",
        "message": message,
        "text": error.to_string(),
        "retryable": error.retryable,
        "provider": PROVIDER,
        "before_request": error.before_request,
        "reason": reason.as_str(),
    })
}

/// A case named `name` that makes `call`, has `exchanges` and fails with
/// `error`, which a replay compares as `compared` says.
fn failing(name: &str, call: &str, exchanges: &Value, error: &Value, compared: &Value) -> Value {
    let mut case = case(name, "failure", call, exchanges, &json!({"error": error}));
    case["compare"] = compared.clone();
    case
}

/// How a replay compares a failure of the contract's: the attributes it
/// reproduces, `beside` after them, and whose `words` its message holds.
fn compared(beside: &[&str], words: &str) -> Value {
    let exactly: Vec<&str> = EXACT.iter().chain(beside).copied().collect();
    json!({"exactly": exactly, "message": words})
}

/// The cases that answer: whole, a listing, the two streams, and a stream
/// whose reads cut inside a character.
fn answers() -> Vec<Value> {
    let mut whole = answered("answered", "answer");
    whole["note"] = "The plain case.".into();
    whole["supports_vision"] = true.into();
    whole["capability"] = true.into();
    whole["catalogue"] =
        json!({"model": "model-a", "known": true, "tags": ["tools"], "reasoning_floor": null});
    let page = answer(200, &json!({"data": [{"id": "model-a"}]}));
    let listing = json!([exchange(got(&format!("{BASE_URL}/models")), page)]);
    let models = json!({"models": [serde_json::to_value(ModelInfo::new("model-a")).unwrap()]});
    let said = json!(["data: {\"said\": \"Yes.\"}\n\n"]);
    let streamed = json!([exchange(
        posted("/answers"),
        reads(&said, "the stream ends")
    )]);
    let events = [
        StreamEvent::TextDelta(StreamTextDelta::new("Yes.")),
        StreamEvent::Done(StreamDone::default()),
    ];
    let events = json!({"events": events.iter().map(Form::written).collect::<Value>()});
    // `data: é`, then a blank line: the first read ends inside the letter.
    let cut = json!([{"hex": "646174613a20c3"}, {"hex": "a90a0a"}]);
    let cut = json!([exchange(posted("/answers"), reads(&cut, "the stream ends"))]);
    vec![
        whole,
        case("listed", "listing", "list_models", &listing, &models),
        case(
            "streamed",
            "stream",
            "generate_structured_stream",
            &streamed,
            &events,
        ),
        case(
            "streamed_as_text",
            "stream",
            "generate_stream",
            &streamed,
            &json!({"text": ["Yes."]}),
        ),
        case(
            "cut_inside_a_character",
            "stream_cut_inside_a_character",
            "generate_stream",
            &cut,
            &json!({"text": ["é"]}),
        ),
    ]
}

/// The cases whose request or whose result holds something of its own: a
/// key the request carries, a request the contract does not read, an id
/// Python minted, and a behaviour to decide.
fn of_their_own() -> Vec<Value> {
    let mut keyed = answered("keyed", "answer");
    keyed["request"]["metadata"]["api_key"] = "**********".into();
    keyed["request_api_key"] = LENT_KEY.into();
    let mut numbered = answered("numbered", "answer");
    numbered["request"]["metadata"]["api_key"] = 7.into();
    numbered["request_api_key"] = 7.into();
    let mut unread = answered("unread", "answer");
    unread["request"] = json!({"messages": "none"});
    let mut minted = answered("minted", "minted");
    let mut called = AiResponse::new("");
    called.tool_calls.push(AiToolCall::new(MINTED, "lookup"));
    minted["expected"]["response"] = called.written();
    minted["minted_call_ids"] = json!([MINTED]);
    let mut to_decide = answered("to_decide", "answer");
    to_decide["questioned"] = "A difference to decide: the answer is kept.".into();
    vec![keyed, numbered, unread, minted, to_decide]
}

/// The cases the SDK decides: a request tried again after a wait, and a
/// redirect followed, which the fixture questions.
fn the_sdk_s() -> Vec<Value> {
    let mut first = exchange(posted("/answers"), answer(500, &json!({})));
    first["wait"] = json!({"seconds": 0.5, "from": "the backoff"});
    let second = exchange(posted("/answers"), answer(200, &json!({"said": "Yes."})));
    let tries = json!([first, second]);
    let mut tried_again = case(
        "tried_again",
        "retry",
        "generate",
        &tries,
        &response("Yes."),
    );
    tried_again["sdk"] = "The SDK tries again.".into();
    let mut moved = answer(302, &json!({}));
    moved["headers"]["location"] = "/elsewhere".into();
    let followed = exchange(
        got("https://llm.example/elsewhere"),
        answer(200, &json!({"said": "Yes."})),
    );
    let hops = json!([exchange(posted("/answers"), moved), followed]);
    let mut redirected = case(
        "redirected",
        "redirect",
        "generate",
        &hops,
        &response("Yes."),
    );
    redirected["sdk"] = "The SDK follows the redirect.".into();
    redirected["questioned"] = "A possible defect: the redirect is followed.".into();
    vec![tried_again, redirected]
}

/// The cases that fail: an answer the server refuses, a response schema's
/// failure, a raw Python exception, a refusal before any request, a stream
/// lost under way, a request that gets no answer, and a failure in the
/// SDK's words.
fn failures() -> Vec<Value> {
    let error = |message: &str, status, retryable| {
        provider_error(PROVIDER, message, status, retryable, false)
    };
    let refusal = json!({"error": {"message": "the key is refused"}});
    let refused = json!([exchange(posted("/answers"), answer(401, &refusal))]);
    let schema = schema_error("the answer is no JSON", ResponseSchemaFailure::InvalidJson);
    let raw = json!({"class": "TypeError", "message": "no such member", "text": "no such member"});
    let unbuilt = provider_error(PROVIDER, "the model is not served", None, false, true);
    let said = json!(["data: {\"said\": \"Yes.\"}\n\n"]);
    let lost = json!([exchange(
        posted("/answers"),
        reads(&said, "the connection is lost")
    )]);
    let mut cut = error("the answer was cut", None, true);
    cut["raised_from"] = "ReadError".into();
    let events = [StreamEvent::TextDelta(StreamTextDelta::new("Yes."))];
    let mut lost = failing(
        "lost_under_way",
        "generate_structured_stream",
        &lost,
        &cut,
        &compared(&["events"], "transport"),
    );
    lost["expected"]["events"] = events.iter().map(Form::written).collect::<Value>();
    let no_answer =
        |failure: &str| json!([exchange(posted("/answers"), json!({"failure": failure}))]);
    let mut late = error("no answer came", None, true);
    late["raised_from"] = "ReadTimeout".into();
    let mut sdk_said = failing(
        "said_by_the_sdk",
        "generate",
        &no_answer("connection refused"),
        &error("Connection error.", None, true),
        &compared(&[], "sdk"),
    );
    sdk_said["sdk"] = "The SDK words the failure.".into();
    vec![
        failing(
            "refused",
            "generate",
            &refused,
            &error("the key is refused", Some(401), false),
            &compared(&[], "server"),
        ),
        failing(
            "off_the_schema",
            "generate",
            &asked_once(),
            &schema,
            &compared(&["reason"], "roomkit"),
        ),
        failing(
            "raw",
            "generate",
            &asked_once(),
            &raw,
            &json!({"exactly": [], "message": "python"}),
        ),
        failing(
            "unbuilt",
            "generate",
            &json!([]),
            &unbuilt,
            &compared(&[], "roomkit"),
        ),
        lost,
        failing(
            "silent",
            "generate",
            &no_answer("read timeout"),
            &late,
            &compared(&[], "transport"),
        ),
        sdk_said,
    ]
}

/// The sound fixture.
pub(super) fn fixture() -> Value {
    let cases = [answers(), of_their_own(), the_sdk_s(), failures()].concat();
    json!({
        "roomkit_version": "0.1.0",
        "roomkit_commit": "0123456789abcdef0123456789abcdef01234567",
        "roomkit_dirty": false,
        "vendor_version": "1.2.3",
        "pydantic_version": "2.0.0",
        "httpx2_version": "1.0.0",
        "python_version": "3.12.0",
        "clock": "2026-01-01T00:00:00Z",
        "notes": {
            "environment": "VENDOR_API_KEY is removed before a case runs.",
            "exchanges.response": "A read that is no UTF-8 is written under hex.",
            "questioned": "A possible defect, or A difference to decide.",
        },
        "questioned": ["redirected", "to_decide"],
        "cases": cases,
    })
}
