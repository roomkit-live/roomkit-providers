//! How the failures every provider words alike read once they left, and
//! what the exit does of a failure a provider words itself.

use roomkit_providers_llm::ProviderError;
use serde_json::{Value, json};

use super::*;

/// The key of a test's provider: a placeholder.
const KEY: &str = "placeholder-not-a-key";

/// What Python's client says of an error inside a stream that brings no
/// message of its own.
const STREAM_ERROR: &str = "An error occurred during streaming";

/// The exit of a test's provider, whose key is the placeholder.
fn test_exit() -> Exit {
    Exit::new("test", Secrets::of([KEY.to_owned()]))
}

// Python: provider_error on the error a stream carried: the status it
// names decides, else its words.
#[test]
fn an_error_inside_a_stream_reads_as_its_status_or_its_words() {
    let read = |error: Value| {
        let error = test_exit().leave(&Failure::stream_error(&error, STREAM_ERROR));
        (error.status_code, error.retryable, error.message)
    };
    let named = |status: Option<u16>, retryable: bool, message: &str| {
        (status, retryable, message.to_owned())
    };
    assert_eq!(
        read(json!({"message": "The server had an error.", "type": "server_error"})),
        named(Some(500), true, "The server had an error.")
    );
    assert_eq!(
        read(json!({"message": "No such model.", "code": 404})),
        named(Some(404), false, "No such model.")
    );
    assert_eq!(
        read(json!({"message": "Rate limit reached"})),
        named(None, true, "Rate limit reached")
    );
    assert_eq!(
        read(json!({"message": "The token limit was reached."})),
        named(None, false, "The token limit was reached.")
    );
    assert_eq!(
        read(json!("rate limit")),
        named(None, false, "An error occurred during streaming")
    );
    assert_eq!(
        read(json!({"message": "key placeholder-not-a-key"})),
        named(None, false, "key [***]")
    );
}

#[test]
fn an_error_object_reads_as_the_status_it_names() {
    let read = |body: Value| {
        let error = test_exit().leave(&Failure::error_object(&body));
        (error.status_code, error.retryable, error.message)
    };
    let named = |status: Option<u16>, retryable: bool, message: &str| {
        (status, retryable, message.to_owned())
    };
    assert_eq!(
        read(json!({"message": "Overloaded.", "type": "overloaded_error"})),
        named(Some(529), true, "Overloaded.")
    );
    assert_eq!(
        read(json!({"message": "Later.", "code": 503})),
        named(Some(503), true, "Later.")
    );
    assert_eq!(
        read(json!({"message": "Slow.", "code": "429"})),
        named(Some(429), true, "Slow.")
    );
    assert_eq!(
        read(json!({"message": "Quota.", "status": "RESOURCE_EXHAUSTED"})),
        named(Some(429), true, "Quota.")
    );
    assert_eq!(
        read(json!("the model is loading")),
        named(None, false, "the model is loading")
    );
    assert_eq!(
        read(json!({"code": true, "type": 200})),
        named(None, false, r#"{"code":true,"type":200}"#)
    );
    let nested = json!({"message": "outer", "error": {"message": "inner", "code": 404}});
    assert_eq!(read(nested), named(Some(404), false, "inner"));
}

// A redirect is a failure whatever its body says: final, with its status.
#[test]
fn a_redirect_is_final_and_keeps_its_status() {
    let redirect = test_exit().leave(&Failure::redirect(307));
    assert_eq!(
        (redirect.retryable, redirect.status_code),
        (false, Some(307))
    );
    assert_eq!(
        redirect.message,
        "the endpoint answered with a redirect (307), which is not followed"
    );
}

/// A failure in a provider's own wording: its own words around what a
/// server `echoed`, of which it carries 40 characters at most.
fn worded(echoed: &str) -> Failure {
    let said = vec![
        own("the server refused the placeholder-not-a-key: "),
        told(echoed, 40),
        own("."),
    ];
    Failure::said(said, false, Some(400))
}

/// What `failure` reads as once it left through `exit`.
fn left(exit: &Exit, failure: &Failure) -> ProviderError {
    exit.leave(failure)
}

// What a server said is cleaned where the failure leaves: no secret, one
// line, no character a terminal acts on, within the bound its piece gives
// it. The provider's own words are carried as they are, never searched for
// a secret; and the failure carries the provider's label.
#[test]
fn what_was_told_is_cleaned_and_bounded_and_the_provider_s_own_words_are_not() {
    let own_words = "the server refused the placeholder-not-a-key: ";
    let echoed = format!("bad\u{1b}[31m key\r\n {KEY}\u{202e} here");
    let error = left(&test_exit(), &worded(&echoed));
    assert_eq!(
        error.message,
        format!("{own_words}bad [31m key [***] here.")
    );
    assert_eq!(
        (error.provider.as_str(), error.status_code, error.retryable),
        ("test", Some(400), false)
    );
    let long = left(&test_exit(), &worded(&"x".repeat(4 * MESSAGE_LIMIT)));
    assert_eq!(long.message, format!("{own_words}{}.", "x".repeat(40)));
    // The whole text is bounded too, whatever its pieces allow.
    let wordy = vec![told("y".repeat(4 * MESSAGE_LIMIT), 4 * MESSAGE_LIMIT)];
    let wordy = left(&test_exit(), &Failure::said(wordy, false, None));
    assert_eq!(wordy.message, "y".repeat(MESSAGE_LIMIT));
    // A failure says that the context overflowed only once told so.
    assert_eq!(error.context_overflow, None);
    let overflowing = left(&test_exit(), &worded("too long").overflowing());
    assert_eq!(overflowing.context_overflow, Some(true));
}

// A request may bring a secret of its own, a key a host lends it: the exit
// of that request removes it beside the configuration's, and the
// provider's exit, which other requests leave through, is as it was.
#[test]
fn a_secret_of_one_request_is_removed_beside_the_configuration_s() {
    let lent = "placeholder-of-one-request";
    let echoed = format!("{KEY} then {lent}");
    let exit = test_exit();
    let of_the_request = exit.with_secrets([lent.to_owned()]);
    assert_eq!(
        left(&of_the_request, &worded(&echoed)).message,
        "the server refused the placeholder-not-a-key: [***] then [***]."
    );
    assert_eq!(
        left(&exit, &worded(&echoed)).message,
        format!("the server refused the placeholder-not-a-key: [***] then {lent}.")
    );
    // A failure the contract built is cleaned of both too.
    let built = ProviderError::new(
        echoed,
        roomkit_providers_llm::ProviderErrorOptions::default(),
    );
    let carried = of_the_request.leave(&Failure::from(built));
    assert_eq!(carried.message, "[***] then [***]");
}
