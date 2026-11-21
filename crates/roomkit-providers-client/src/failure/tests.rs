//! How the failures every provider words alike read once they left, and
//! what the exit does of a failure a provider words itself.

use roomkit_providers_llm::{
    API_KEY_METADATA_KEY, AiRequest, ProviderError, ProviderErrorOptions, repr,
};
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

// A provider that could not be built has no client yet, and so no secret to
// clean a text of: its failure leaves in the provider's own words, as they
// are, with its label. Should such a failure quote a server or a reader,
// the mark stands where the quote was: nothing told leaves that way.
#[test]
fn the_failure_of_a_provider_that_is_not_built_leaves_in_its_own_words_alone() {
    let unbuilt = Exit::unbuilt("test", &Failure::unbuilt("its base URL names no host"));
    assert_eq!(
        unbuilt.message,
        "the provider cannot be built: its base URL names no host"
    );
    assert_eq!(
        (
            unbuilt.provider.as_str(),
            unbuilt.before_request,
            unbuilt.retryable
        ),
        ("test", true, false)
    );
    let quoting = Exit::unbuilt("test", &worded(&format!("no {KEY} here")).overflowing());
    assert_eq!(
        quoting.message,
        "the server refused the placeholder-not-a-key: [***]."
    );
    assert_eq!(
        (quoting.status_code, quoting.context_overflow),
        (Some(400), Some(true))
    );
    // A failure the contract built is one piece: it leaves as it is.
    let built = ProviderError::new(
        "a\tcontract's\nwords".to_owned(),
        ProviderErrorOptions::default(),
    );
    let carried = Exit::unbuilt("test", &Failure::from(built));
    assert_eq!(carried.message, "a contract's words");
}

/// A request that brings `key` for itself.
fn lending(key: &str) -> AiRequest {
    let mut request = AiRequest::default();
    let lent = (API_KEY_METADATA_KEY.to_owned(), key.into());
    request.metadata.extend([lent]);
    request
}

// A request may bring a secret of its own, a key a host lends it: the
// failure of its generation leaves with it, without that key beside the
// configuration's. What leaves without a request is as it was, and so is
// the failure of a request that brings no key.
#[test]
fn the_key_a_request_brings_is_removed_beside_the_configuration_s() {
    let lent = "placeholder-of-one-request";
    let echoed = format!("{KEY} then {lent}");
    let (exit, request) = (test_exit(), lending(lent));
    assert_eq!(
        exit.leave_for(&request, &worded(&echoed)).message,
        "the server refused the placeholder-not-a-key: [***] then [***]."
    );
    let as_it_was = format!("the server refused the placeholder-not-a-key: [***] then {lent}.");
    assert_eq!(left(&exit, &worded(&echoed)).message, as_it_was);
    for no_key in [AiRequest::default(), lending("")] {
        let error = exit.leave_for(&no_key, &worded(&echoed));
        assert_eq!(error.message, as_it_was);
    }
    // A failure the contract built is cleaned of both too.
    let built = ProviderError::new(echoed, ProviderErrorOptions::default());
    let carried = exit.leave_for(&request, &Failure::from(built));
    assert_eq!(carried.message, "[***] then [***]");
}

// The key a request brings is searched for in every form a secret of the
// configuration is: as a server sent it, as the code that builds a failure
// writes it again, with other white space, split by a character that shows
// nothing; in a failure a provider words as in one built elsewhere.
#[test]
fn the_key_a_request_brings_is_hidden_in_every_form_a_configured_secret_is() {
    let lent = "placeholder \"of\" one\\request's\tkey";
    let forms = [
        lent.to_owned(),
        serde_json::to_string(lent).unwrap(),
        format!("{lent:?}"),
        repr(&lent.into()),
        repr(&format!("{lent}\"").into()),
        lent.replace(' ', "\n  "),
        lent.replace("one", "o\u{200b}ne"),
    ];
    let configured = Exit::new("test", Secrets::of([lent.to_owned()]));
    let (bare, request) = (Exit::new("test", Secrets::default()), lending(lent));
    for form in forms {
        let echoed = format!("refused: {form} !");
        let worded = Failure::said(vec![told(echoed.clone(), MESSAGE_LIMIT)], false, Some(401));
        let built = ProviderError::new(echoed, ProviderErrorOptions::default());
        for failure in [worded, Failure::from(built)] {
            let hidden = bare.leave_for(&request, &failure);
            assert_eq!(hidden.message, configured.leave(&failure).message);
            let printed = format!("{hidden} {hidden:?}");
            assert!(
                printed.contains(MARK) && !printed.contains("placeholder"),
                "{form}: {printed}"
            );
            // Without the request, the key is a text like another.
            assert!(bare.leave(&failure).message.contains("placeholder"));
        }
    }
}

mod alike;
mod secrets;
