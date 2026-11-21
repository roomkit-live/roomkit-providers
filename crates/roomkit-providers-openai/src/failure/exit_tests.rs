//! What a failure's text holds when it leaves the provider, whoever built
//! the failure: no secret of the configuration, no character a terminal
//! acts on, one line, within its bound. Each is asked of the provider
//! itself, whole and streamed.

use std::fmt::Write as _;

use roomkit_providers_core::{AiProvider, AiRequest, ProviderError, ResponseSchemaFailure};
use serde_json::{Value, json};

use super::MESSAGE_LIMIT;
use crate::OpenAiConfig;
use crate::ported::support::{
    KEY, chunk, completion, elsewhere, events, hello, json_reply, pointed, schema_failure,
};
use crate::replay::drained;
use crate::replay::fake::{Fake, Reply};

/// A question under a schema of one required text.
fn asking() -> AiRequest {
    let schema = json!({
        "type": "object",
        "properties": {"label": {"type": "string"}},
        "required": ["label"],
        "additionalProperties": false,
    });
    AiRequest {
        response_schema: schema.as_object().cloned(),
        ..hello()
    }
}

/// A whole answer and a streamed one whose message, or whose one delta,
/// holds `members`.
fn answers(members: &Value) -> [Reply; 2] {
    let end = json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]});
    [completion(members, "stop"), events(&[chunk(members), end])]
}

/// The failures of `request` asked of the provider of `config`, whole of
/// the first of `replies` and streamed of the second.
async fn failures(
    config: &OpenAiConfig,
    replies: [Reply; 2],
    request: &AiRequest,
) -> [ProviderError; 2] {
    let fake = Fake::serving(replies.to_vec()).await;
    let provider = pointed(config.clone(), &fake);
    let whole = provider.generate(request).await.unwrap_err();
    let (_, streamed) = drained(provider.generate_structured_stream(request)).await;
    [whole, streamed.unwrap()]
}

/// Everything a host can print of `error`: its text, its `Debug` form and
/// each of its sources.
fn printed(error: &ProviderError) -> String {
    let mut printed = format!("{error} {error:?}");
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        write!(printed, " {cause} {cause:?}").unwrap();
        source = cause.source();
    }
    printed
}

fn with_key(key: &str) -> OpenAiConfig {
    OpenAiConfig {
        api_key: key.to_owned(),
        ..elsewhere("local-model")
    }
}

/// A refusal of HTTP `status` whose error object is `error`, whole and as
/// the answer to a stream.
fn refused(status: u16, error: &Value) -> [Reply; 2] {
    let body = json!({"error": error}).to_string();
    [json_reply(status, body.clone()), json_reply(status, body)]
}

// A refusal the server words with the key, 5,000 characters long, under a
// response schema: the contract's check builds the failure.
#[tokio::test]
async fn a_schema_s_failure_carries_no_key_and_is_bounded() {
    let refusal = format!("I cannot say {KEY}.\n{}", "x".repeat(5000));
    let replies = answers(&json!({"role": "assistant", "content": "", "refusal": refusal}));
    for error in failures(&with_key(KEY), replies, &asking()).await {
        assert_eq!(schema_failure(&error), Some(ResponseSchemaFailure::Refusal));
        assert!(!printed(&error).contains(KEY), "{}", printed(&error));
        assert_eq!(error.message.chars().count(), MESSAGE_LIMIT);
        assert!(
            error
                .message
                .starts_with("the model declined to answer: I cannot say [***]. xxx"),
            "{}",
            error.message
        );
    }
}

// An answer whose unexpected member is named by the key: the mismatch names
// the member.
#[tokio::test]
async fn a_schema_s_mismatch_names_no_key() {
    let content = json!({"label": "yes", KEY: 1}).to_string();
    let replies = answers(&json!({"role": "assistant", "content": content}));
    for error in failures(&with_key(KEY), replies, &asking()).await {
        assert_eq!(
            schema_failure(&error),
            Some(ResponseSchemaFailure::InvalidJson)
        );
        assert!(!printed(&error).contains(KEY), "{}", printed(&error));
        assert!(error.message.contains("[***]"), "{}", error.message);
    }
}

// A key too short to be a secret hides nothing and shreds nothing: the
// code is compared as the server wrote it, and the provider's own words
// are not searched for a key.
#[tokio::test]
async fn a_short_key_hides_no_overflow_and_shreds_no_message() {
    let error = json!({
        "message": "Too long: excess text.",
        "type": "invalid_request_error",
        "code": "context_length_exceeded",
    });
    for key in [
        "x",
        "e",
        "context",
        "provider",
        "answered",
        "length_exceeded",
    ] {
        let [whole, streamed] = failures(&with_key(key), refused(400, &error), &hello()).await;
        for failure in [whole, streamed] {
            assert_eq!(failure.context_overflow, Some(true), "{key}");
            let own = "the provider answered HTTP 400 (invalid_request_error, ";
            assert!(
                failure.message.starts_with(own),
                "{key}: {}",
                failure.message
            );
            assert!(
                failure.message.ends_with("): Too long: excess text."),
                "{key}"
            );
        }
    }
}

// A key of eight characters or more is removed from what a server said; a
// shorter one is no secret.
#[tokio::test]
async fn a_key_is_removed_from_eight_characters() {
    for (key, shown) in [("seven77", "seven77"), ("eight888", "[***]")] {
        let error = json!({"message": format!("Bearer {key} is refused")});
        for failure in failures(&with_key(key), refused(401, &error), &hello()).await {
            assert_eq!(
                failure.message,
                format!("the provider answered HTTP 401: Bearer {shown} is refused")
            );
        }
    }
}

// Removing a secret leaves no secret behind, whatever the text around it.
#[tokio::test]
async fn a_text_built_around_a_key_holds_no_key_once_cleaned() {
    let key = "ab[***]cd";
    // The key around itself, four deep: each removal makes the next.
    let nested = format!("{}[***]{}", "ab".repeat(4), "cd".repeat(4));
    let error = json!({"message": format!("{nested} then ab\n[***]cd and ab[**ab[***]cd*]cd")});
    for failure in failures(&with_key(key), refused(401, &error), &hello()).await {
        assert!(!failure.message.contains(key), "{}", failure.message);
    }
}

const OVERFLOW: &str = "This model's maximum context length is 128000 tokens. However, your \
     messages resulted in 130000 tokens.";

// An overflow is decided from everything the server said, whatever part
// of it the failure echoes.
#[tokio::test]
async fn an_overflow_said_anywhere_in_the_body_is_an_overflow() {
    let json = |body: Value| json_reply(400, body.to_string());
    let cases = [
        (
            json(json!({"detail": OVERFLOW})),
            Some(true),
            "the provider answered HTTP 400: Bad Request",
        ),
        (
            json(json!({"error": OVERFLOW})),
            Some(true),
            "the provider answered HTTP 400: This model's maximum context length is 128000 tokens. However, your messages resulted in 130000 tokens.",
        ),
        (
            json_reply(400, OVERFLOW),
            Some(true),
            "the provider answered HTTP 400: Bad Request",
        ),
        (
            json(json!({"error": {"message": "No.", "code": "invalid_api_key"}})),
            None,
            "the provider answered HTTP 400 (invalid_api_key): No.",
        ),
        (
            json(json!({"detail": "The request is not valid."})),
            None,
            "the provider answered HTTP 400: Bad Request",
        ),
        (
            json_reply(400, "Not valid."),
            None,
            "the provider answered HTTP 400: Bad Request",
        ),
    ];
    for (reply, overflow, said) in cases {
        let replies = [reply.clone(), reply];
        for failure in failures(&with_key(KEY), replies, &hello()).await {
            assert_eq!(failure.context_overflow, overflow, "{said}");
            assert_eq!(failure.message, said);
        }
    }
}

/// Whether a failure's text may hold `character`: no control character and
/// no control of the direction of a text.
fn may_leave(character: char) -> bool {
    let directional = matches!(character, '\u{61c}' | '\u{200e}' | '\u{200f}')
        || ('\u{202a}'..='\u{202e}').contains(&character)
        || ('\u{2066}'..='\u{2069}').contains(&character);
    !character.is_control() && !directional
}

// What a terminal acts on reaches no failure: each control character and
// each control of the direction of a text becomes a space, and spaces are
// one.
#[tokio::test]
async fn no_control_character_leaves_in_a_failure() {
    let said = "bad\u{1b}[31m red\u{7}\u{0}\u{7f}\u{9c} \u{202e}txet\u{2066}\u{2069} end\u{200f}\u{61c}\r\n\tlast";
    let cleaned = "bad [31m red txet end last";
    // A status's error object, whole and streamed.
    for failure in failures(
        &with_key(KEY),
        refused(400, &json!({"message": said, "type": said})),
        &hello(),
    )
    .await
    {
        assert_eq!(
            failure.message,
            format!("the provider answered HTTP 400 ({cleaned}): {cleaned}")
        );
    }
    // An error object in place of an answer, and an error inside a stream.
    let object = json!({"error": {"message": said}});
    let replies = [
        json_reply(200, object.to_string()),
        events(std::slice::from_ref(&object)),
    ];
    for failure in failures(&with_key(KEY), replies, &hello()).await {
        assert_eq!(failure.message, cleaned);
    }
    // A refusal under a schema.
    let replies = answers(&json!({"role": "assistant", "content": "", "refusal": said}));
    for failure in failures(&with_key(KEY), replies, &asking()).await {
        assert_eq!(
            failure.message,
            format!("the model declined to answer: {cleaned}")
        );
        assert!(
            printed(&failure).chars().all(may_leave),
            "{:?}",
            printed(&failure)
        );
    }
}

// Every secret the configuration holds is removed from what a server
// said: the key, the value of each default header, and what the client
// sends of a base URL's user information.
#[tokio::test]
async fn no_secret_of_the_configuration_leaves_in_a_failure() {
    let headers = [
        ("x-gateway-token", "gateway-secret-1"),
        ("proxy-authorization", "Basic Z2F0ZXdheTpzZWNyZXQ="),
        ("x-tenant", "blue"),
    ];
    let config = OpenAiConfig {
        base_url: Some("http://user-name-1:pass-word-1@llm.example/v1".to_owned()),
        default_headers: Some(
            headers
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .into(),
        ),
        ..with_key(KEY)
    };
    let said = format!(
        "bad credentials: Basic Z2F0ZXdheTpzZWNyZXQ=, token Z2F0ZXdheTpzZWNyZXQ=, \
         gateway-secret-1, {KEY}, user-name-1:pass-word-1, pass-word-1, \
         Basic dXNlci1uYW1lLTE6cGFzcy13b3JkLTE=, tenant blue"
    );
    for failure in failures(&config, refused(401, &json!({"message": said})), &hello()).await {
        assert_eq!(
            failure.message,
            "the provider answered HTTP 401: bad credentials: [***], token [***], [***], \
             [***], [***], [***], [***], tenant blue"
        );
    }
}

// A base URL's user information is sent as Python's client sends it: as
// Basic credentials, in place of the key.
#[tokio::test]
async fn a_base_url_s_user_information_is_sent_as_basic_credentials() {
    for key in [KEY, ""] {
        let config = OpenAiConfig {
            base_url: Some("http://user:password@llm.example/v1".to_owned()),
            ..with_key(key)
        };
        let fake = Fake::serving(vec![json_reply(404, "{}"), json_reply(404, "{}")]).await;
        let provider = pointed(config, &fake);
        provider.generate(&hello()).await.unwrap_err();
        provider.list_models().await.unwrap_err();
        for seen in fake.seen() {
            assert_eq!(
                seen.headers.get("authorization").map(String::as_str),
                Some("Basic dXNlcjpwYXNzd29yZA=="),
                "{key:?}"
            );
            assert!(!seen.target.contains("user"), "{}", seen.target);
        }
    }
}
