//! What a failure's text holds when it leaves the provider, whoever built
//! the failure: no secret of the configuration, no character a terminal
//! acts on, one line, within its bound. Each is asked of the provider
//! itself, whole and streamed.

use std::fmt::Write as _;

use roomkit_providers_llm::{AiProvider, AiRequest, ProviderError, ResponseSchemaFailure};
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
        // The code alone says so, written with an escape: the body's text
        // does not hold the code as it reads.
        (
            json_reply(
                400,
                r#"{"error": {"message": "No.", "code": "context\u005flength_exceeded"}}"#,
            ),
            Some(true),
            "the provider answered HTTP 400 (context_length_exceeded): No.",
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
    let shows_nothing = matches!(character, '\u{ad}' | '\u{61c}' | '\u{180e}' | '\u{feff}')
        || ('\u{200b}'..='\u{200f}').contains(&character)
        || ('\u{202a}'..='\u{202e}').contains(&character)
        || ('\u{2060}'..='\u{206f}').contains(&character)
        || ('\u{fff9}'..='\u{fffb}').contains(&character)
        || ('\u{e0000}'..='\u{e007f}').contains(&character);
    !character.is_control() && !shows_nothing
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

/// A configuration whose key is `secret`.
fn keyed(secret: &str) -> OpenAiConfig {
    with_key(secret)
}

// A secret that a JSON string, a reader's quote or the contract's quote
// writes otherwise than it is leaves in none of those forms. Each of the
// four ways a failure quotes an answer is asked: an error object with no
// message, the JSON reader's refusal of a member, a call's id that is a
// list, and the member a response schema does not expect. The secrets are
// such that each form is the only one that finds one of them: a control
// character is written three ways (`\u0001` in JSON, `\u{1}` by the
// reader, `\x01` by the contract), and the contract escapes a single
// quote only beside a double one.
#[tokio::test]
async fn a_secret_leaves_in_no_form_a_failure_writes_it_in() {
    // A password with a control character, which a key cannot hold.
    let behind = OpenAiConfig {
        base_url: Some("http://user:pass%01word-1x@llm.example/v1".to_owned()),
        ..with_key(KEY)
    };
    let secrets = [
        (keyed("pa\"ss\\word1x"), "pa\"ss\\word1x", "word1x"),
        (keyed("it's a\tsecret1"), "it's a\tsecret1", "secret1"),
        (
            keyed("both ' and \" quotes1"),
            "both ' and \" quotes1",
            "quotes1",
        ),
        (
            keyed("line\u{2028}break-secret"),
            "line\u{2028}break-secret",
            "break-secret",
        ),
        (
            keyed("it's\u{2028}secret-1x"),
            "it's\u{2028}secret-1x",
            "secret-1x",
        ),
        (
            keyed("{\"k\":\"abcdefgh\"}"),
            "{\"k\":\"abcdefgh\"}",
            "abcdefgh",
        ),
        (behind, "pass\u{1}word-1x", "word-1x"),
    ];
    for (config, secret, piece) in secrets {
        let fits = |error: &ProviderError| {
            let shown = printed(error);
            assert!(!shown.contains(piece), "{secret:?}: {shown}");
            assert!(
                error.message.contains("[***]"),
                "{secret:?}: {}",
                error.message
            );
        };
        // An error object with no message, written as JSON.
        let object = json!({"error": {"detail": format!("refused: {secret}")}});
        let replies = [
            json_reply(200, object.to_string()),
            events(std::slice::from_ref(&object)),
        ];
        let [whole, _] = failures(&config, replies, &hello()).await;
        fits(&whole);
        // The JSON reader's refusal, and a call's id that is a list.
        let index = json!({"tool_calls": [{"index": secret, "function": {"name": "now"}}]});
        let id = json!({"tool_calls": [{"index": 0, "id": [secret], "function": {"name": "now"}}]});
        for delta in [index, id] {
            let fake = Fake::serving(vec![events(&[chunk(&delta)])]).await;
            let provider = pointed(config.clone(), &fake);
            let (_, failure) = drained(provider.generate_structured_stream(&hello())).await;
            fits(&failure.unwrap());
        }
        // The member a response schema does not expect, whole and streamed:
        // named by the secret, then by the secret between double quotes.
        for member in [secret.to_owned(), format!("\"{secret}\"")] {
            let content = json!({"label": "yes", member: 1}).to_string();
            let replies = answers(&json!({"role": "assistant", "content": content}));
            for failure in failures(&config, replies, &asking()).await {
                fits(&failure);
            }
        }
    }
}

// A character that shows nothing is dropped before the secrets are looked
// for: a secret a server sends back with one inside is still a secret.
#[tokio::test]
async fn a_secret_split_by_a_character_that_shows_nothing_is_removed() {
    let invisible = [
        '\u{ad}',
        '\u{61c}',
        '\u{180e}',
        '\u{200b}',
        '\u{200c}',
        '\u{200d}',
        '\u{200e}',
        '\u{200f}',
        '\u{202a}',
        '\u{202e}',
        '\u{2060}',
        '\u{2064}',
        '\u{2066}',
        '\u{2069}',
        '\u{206f}',
        '\u{feff}',
        '\u{fff9}',
        '\u{fffb}',
        '\u{e0001}',
        '\u{e0041}',
        '\u{e007f}',
    ];
    for hidden in invisible {
        let (head, tail) = KEY.split_at(9);
        let split =
            format!("Bearer {hidden}{head}{hidden}{hidden}{tail}{hidden} is{hidden} refused");
        let error = json!({"message": split});
        for failure in failures(&with_key(KEY), refused(401, &error), &hello()).await {
            assert_eq!(
                failure.message,
                "the provider answered HTTP 401: Bearer [***] is refused",
                "U+{:04X}",
                u32::from(hidden)
            );
        }
    }
}

// Each form of a base URL's user information is a secret: as it is
// written, as it reads once decoded, each part alone and the two joined.
#[tokio::test]
async fn each_form_of_a_base_url_s_user_information_is_removed() {
    let config = OpenAiConfig {
        base_url: Some("http://us%40er-name1:pa%73%73word-1@llm.example/v1".to_owned()),
        ..with_key(KEY)
    };
    let forms = [
        "us%40er-name1",
        "pa%73%73word-1",
        "us%40er-name1:pa%73%73word-1",
        "us@er-name1",
        "password-1",
        "us@er-name1:password-1",
        "Basic dXNAZXItbmFtZTE6cGFzc3dvcmQtMQ==",
        "dXNAZXItbmFtZTE6cGFzc3dvcmQtMQ==",
    ];
    for form in forms {
        let error = json!({"message": format!("refused: <{form}>")});
        for failure in failures(&config, refused(401, &error), &hello()).await {
            assert_eq!(
                failure.message, "the provider answered HTTP 401: refused: <[***]>",
                "{form}"
            );
        }
    }
    // A user and a password each too short to be a secret are one joined.
    let config = OpenAiConfig {
        base_url: Some("http://u:secret7@llm.example/v1".to_owned()),
        ..with_key(KEY)
    };
    let error = json!({"message": "refused: <u:secret7> then <secret7>"});
    for failure in failures(&config, refused(401, &error), &hello()).await {
        assert_eq!(
            failure.message,
            "the provider answered HTTP 401: refused: <[***]> then <secret7>"
        );
    }
}

// The secrets are looked for before the text is put on one line and after:
// a secret a server sends back with other white space is found after, and
// one whose one-line form is too short to be a secret is found before.
#[tokio::test]
async fn a_secret_is_looked_for_before_and_after_the_text_is_put_on_one_line() {
    let header = |value: &str| OpenAiConfig {
        default_headers: Some([("x-token".to_owned(), value.to_owned())].into()),
        ..with_key(KEY)
    };
    let said = |echoed: &str| json!({"message": format!("refused: <{echoed}>")});
    let cleaned = "the provider answered HTTP 401: refused: <[***]>";
    let cases = [
        ("my secret key1", "my   secret\tkey1"),
        ("my secret key1", "my\r\nsecret key1"),
        ("p \t \t \t w", "p \t \t \t w"),
    ];
    for (secret, echoed) in cases {
        let replies = refused(401, &said(echoed));
        for failure in failures(&header(secret), replies, &hello()).await {
            assert_eq!(failure.message, cleaned, "{secret:?} as {echoed:?}");
        }
    }
}
