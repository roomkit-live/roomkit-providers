//! What a provider crate calls of the contract beside its documented items:
//! the helpers hidden from the documentation, reached from outside the
//! crate as a provider reaches them.

use std::error::Error;
use std::future::Future;
use std::pin::pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::{Context, Poll, Waker};

use roomkit_providers_core::{
    AiRequest, AiResponse, AiToolCall, DataUriError, MockAiProvider, ModelInfo, ObjectOnly,
    ProviderError, ResponseSchemaError, ResponseSchemaFailure, StreamDone, StreamEvent, Yielder,
    check_schema_answer, checked_stream, dumps, items, listing, next, parse_data_uri, read_name,
    read_tool_calls, repr, request_api_key, schema_for_generate,
};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value, json};

/// A schema of one required text.
fn schema() -> Map<String, Value> {
    json!({
        "type": "object",
        "properties": { "label": { "type": "string" } },
        "required": ["label"],
        "additionalProperties": false,
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}

/// The failure of a response schema `error` carries.
fn reason(error: &ProviderError) -> Option<ResponseSchemaFailure> {
    let failure = error.source()?.downcast_ref::<ResponseSchemaError>()?;
    Some(failure.reason)
}

// A Chat Completions wire carries a call's arguments as the text Python's
// `json.dumps` writes.
#[test]
fn a_call_s_arguments_are_written_as_python_s_json_dumps_writes_them() {
    let arguments = json!({ "order": "A-1001", "note": "caf\u{e9}", "ratio": 0.5 });
    assert_eq!(
        dumps(&arguments),
        r#"{"order": "A-1001", "note": "caf\u00e9", "ratio": 0.5}"#
    );
}

// A provider that reads an image's bytes off a `data:` URI itself.
#[test]
fn a_data_uri_reads_as_its_media_type_and_its_bytes() {
    assert_eq!(
        parse_data_uri("data:image/png;base64,QUJD", None),
        Ok(("image/png".to_owned(), b"ABC".to_vec()))
    );
    assert_eq!(
        parse_data_uri("data:;base64,QUJD", Some("image/webp")).map(|(kind, _)| kind),
        Ok("image/webp".to_owned())
    );
    assert_eq!(
        parse_data_uri("https://example.com/a.png", None),
        Err(DataUriError::NotADataUri)
    );
    assert_eq!(
        DataUriError::NotBase64.to_string(),
        "data URI payload is not valid base64"
    );
}

// A provider's `generate` checks the answer it returns.
#[test]
fn a_whole_answer_is_checked_against_its_schema() {
    let checked = |content: &str, refusal: Option<&str>, finish: Option<&str>| {
        check_schema_answer(content, &schema(), "acme", refusal, finish)
    };
    assert_eq!(checked(r#"{"label": "yes"}"#, None, Some("stop")), Ok(()));
    let declined = checked("", Some("unsafe"), Some("stop")).unwrap_err();
    assert_eq!(reason(&declined), Some(ResponseSchemaFailure::Refusal));
    assert_eq!(declined.provider, "acme");
}

// A provider refuses a schema it cannot carry before sending anything, and
// checks a streamed answer before its done event.
#[test]
fn a_request_s_schema_is_refused_or_carried_before_any_request() {
    let request = AiRequest {
        response_schema: Some(schema()),
        ..AiRequest::default()
    };
    assert_eq!(
        schema_for_generate(&request, true, false, "acme"),
        Ok(Some(&schema()))
    );
    let refused = schema_for_generate(&request, false, false, "acme").unwrap_err();
    assert_eq!(reason(&refused), Some(ResponseSchemaFailure::Unsupported));
    assert_eq!(
        schema_for_generate(&AiRequest::default(), false, false, "acme"),
        Ok(None)
    );

    // Without a schema, a stream is handed back untouched.
    let plain = AiRequest::default();
    let provider = MockAiProvider::new(roomkit_providers_core::MockAiProviderOptions::default());
    let events = roomkit_providers_core::AiProvider::generate_structured_stream(&provider, &plain);
    let _untouched = checked_stream(events, &plain, "acme", |_: &StreamDone| None);
}

// A provider reading its wire: its calls, its listing and the credential of
// one request.
#[test]
fn a_wire_s_calls_a_listing_and_a_request_s_key_are_read() {
    let raw = vec![
        (
            "c1".to_owned(),
            "lookup".to_owned(),
            r#"{"order": "42"}"#.to_owned(),
        ),
        (
            "c2".to_owned(),
            "lookup".to_owned(),
            r#"{"order": "#.to_owned(),
        ),
    ];
    let calls = read_tool_calls(raw, Some("length"));
    let marks: Vec<(bool, bool)> = calls
        .iter()
        .map(|call| (call.partial, call.garbled))
        .collect();
    assert_eq!(marks, [(false, false), (true, false)]);
    let garbled = AiToolCall::read("c3", "lookup", "[1]", Some("stop"), true);
    assert!(garbled.partial && garbled.garbled);

    let curated = [ModelInfo {
        context_window: Some(8192),
        ..ModelInfo::new("example-small-1")
    }];
    let listed = listing(vec![ModelInfo::new("example-small-1")], &curated);
    assert_eq!(listed[0].context_window, Some(8192));

    assert_eq!(request_api_key(&AiRequest::default()), None);
    let _whole: Option<AiResponse> = None;
    let _event: Option<StreamEvent> = None;
}

/// What a provider's configuration could look like: a model read from an
/// object only, and a name read from a string only.
#[derive(Debug, PartialEq)]
struct Endpoint {
    api: Api,
    retries: u32,
}

#[derive(Debug, PartialEq)]
enum Api {
    Chat,
    Responses,
}

impl<'de> Deserialize<'de> for Api {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        read_name(
            deserializer,
            "an API, `chat` or `responses`",
            |name| match name {
                "chat" => Some(Self::Chat),
                "responses" => Some(Self::Responses),
                _ => None,
            },
        )
    }
}

#[derive(Deserialize)]
struct EndpointForm {
    api: Api,
    #[serde(default)]
    retries: u32,
}

impl<'de> Deserialize<'de> for Endpoint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let form = EndpointForm::deserialize(ObjectOnly(deserializer))?;
        Ok(Self {
            api: form.api,
            retries: form.retries,
        })
    }
}

// A provider reading its configuration as narrowly as the contract reads a
// model: an object and a name. A list of the fields in order and an object
// of one key, which the readers serde derives also take, are refused.
#[test]
fn a_configuration_is_read_from_an_object_and_a_name_from_a_string() {
    let read = |value: Value| serde_json::from_value::<Endpoint>(value).ok();
    let chat = Endpoint {
        api: Api::Chat,
        retries: 2,
    };
    assert_eq!(read(json!({"api": "chat", "retries": 2})), Some(chat));
    let responses = read(json!({"api": "responses"}));
    assert_eq!(responses.map(|endpoint| endpoint.api), Some(Api::Responses));
    for refused in [
        json!(["chat", 2]),
        json!("chat"),
        json!({"api": {"chat": null}}),
        json!({"api": "other"}),
        json!({"api": 1}),
    ] {
        assert_eq!(read(refused.clone()), None, "{refused}");
    }
}

// A provider writes its stream as a producer that hands its items over, and
// reads another stream item by item: the producer runs only while its
// stream is polled, and never ahead of its reader.
#[test]
fn a_stream_is_written_as_a_producer_and_read_item_by_item() {
    let produced = AtomicU32::new(0);
    let last = &produced;
    let mut stream = pin!(items(|out: Yielder<u32>| async move {
        for item in 1..=3 {
            last.store(item, Ordering::SeqCst);
            out.give(item).await;
        }
    }));
    let mut context = Context::from_waker(Waker::noop());
    let mut read = Vec::new();
    loop {
        let item = pin!(next(&mut stream)).poll(&mut context);
        let Poll::Ready(item) = item else {
            panic!("a producer that waits for nothing is never pending");
        };
        let Some(item) = item else {
            break;
        };
        // The producer stopped at the item it handed over.
        assert_eq!(produced.load(Ordering::SeqCst), item);
        read.push(item);
    }
    assert_eq!(read, [1, 2, 3]);
}

// The contract quotes a value in a failure's text as Python's `repr` writes
// it: a provider that removes a secret from a failure knows the form.
#[test]
fn a_value_is_quoted_in_a_failure_as_python_s_repr_writes_it() {
    let quoted = repr(&json!([
        "pa\\ss",
        "it's",
        "a \"b\"",
        "both ' and \"",
        "tab\there\u{2028}"
    ]));
    assert_eq!(
        quoted,
        r#"['pa\\ss', "it's", 'a "b"', 'both \' and "', 'tab\there\u2028']"#
    );
}
