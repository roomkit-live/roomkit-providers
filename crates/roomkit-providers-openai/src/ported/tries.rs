//! What a retry reaches of a stream and of a model listing: a stream is
//! tried again until the head of its answer came, never after.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use roomkit_providers_core::{AiProvider, ProviderError};
use serde_json::json;

use super::support::{
    KEY, chunk, elsewhere, events, hello, json_reply, pointed, saying, stream_reply, text_of,
};
use crate::exchange::Pace;
use crate::replay::drained;
use crate::replay::fake::{Body, Ending, Fake, Reply};
use crate::{OpenAiAiProvider, OpenAiConfig};

/// A provider behind a base URL that keeps its waits instead of taking
/// them, pointed at a fake that answers with `replies`.
async fn retrying(
    replies: Vec<Reply>,
    config: OpenAiConfig,
) -> (OpenAiAiProvider, Fake, Arc<Mutex<Vec<Duration>>>) {
    let fake = Fake::serving(replies).await;
    let mut provider = pointed(config, &fake);
    let waits = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&waits);
    provider.pace = Pace {
        now: Arc::new(|| 1_000_000.0),
        wait: Arc::new(move |wait| {
            kept.lock().unwrap().push(wait);
            Box::pin(std::future::ready(()))
        }),
    };
    (provider, fake, waits)
}

fn retried(max_retries: u32) -> OpenAiConfig {
    OpenAiConfig {
        max_retries,
        ..elsewhere("local-model")
    }
}

/// The text a generation answered, or its failure, through `generate` or
/// through its stream.
async fn answer(provider: &OpenAiAiProvider, streamed: bool) -> Result<String, ProviderError> {
    let request = hello();
    if !streamed {
        let response = provider.generate(&request).await;
        return response.map(|response| response.content);
    }
    let (events, failure) = drained(provider.generate_structured_stream(&request)).await;
    failure.map_or_else(|| Ok(text_of(&events)), Err)
}

/// A streamed completion that says `text`.
fn saying_streamed(text: &str) -> Reply {
    events(&[chunk(&json!({"content": text}))])
}

/// The head of an answer of HTTP `status`, whose connection closes inside
/// its body.
fn dropped_in_its_body(status: u16, content_type: &str) -> Reply {
    Reply::Answer {
        status,
        headers: vec![("content-type".to_owned(), content_type.to_owned())],
        body: Body::Reads(vec![b"{\"err".to_vec()], Ending::Lost),
    }
}

// Source: tests/agent/retries.rs::a_connection_that_closed_is_tried_again_after_its_backoff
// (there the backoff is taken; here it is kept: at least 0.375 s, then
// at least 0.75 s)
#[tokio::test]
async fn a_connection_that_closed_is_tried_again_after_its_backoff() {
    let replies = vec![Reply::Refused, Reply::Refused, saying_streamed("Recovered")];
    let (provider, fake, waits) = retrying(replies, retried(2)).await;
    assert_eq!(answer(&provider, true).await.unwrap(), "Recovered");
    assert_eq!(fake.seen().len(), 3);
    let waits = waits.lock().unwrap().clone();
    let [first, second] = waits.as_slice() else {
        panic!("two waits: {waits:?}");
    };
    let within = |wait: &Duration, least: u64, most: u64| {
        (Duration::from_millis(least)..=Duration::from_millis(most)).contains(wait)
    };
    assert!(
        within(first, 375, 500) && within(second, 750, 1000),
        "{waits:?}"
    );
}

// Source: tests/agent/retries.rs::a_timeout_is_tried_again
#[tokio::test]
async fn a_timeout_is_tried_again() {
    let replies = vec![Reply::Silent, saying_streamed("Recovered")];
    let timed = OpenAiConfig {
        timeout: Duration::from_millis(50),
        ..retried(1)
    };
    let (provider, fake, waits) = retrying(replies, timed).await;
    assert_eq!(answer(&provider, true).await.unwrap(), "Recovered");
    assert_eq!(fake.seen().len(), 2);
    assert_eq!(waits.lock().unwrap().len(), 1);
}

// Source: tests/agent/retries.rs::the_wait_an_endpoint_asks_is_taken
// (there the wait is taken; here it is kept)
#[tokio::test]
async fn the_wait_an_endpoint_asks_is_taken() {
    let asking = Reply::Answer {
        status: 429,
        headers: vec![("retry-after-ms".to_owned(), "700".to_owned())],
        body: Body::Whole(b"{}".to_vec()),
    };
    let replies = vec![asking, saying_streamed("Recovered")];
    let (provider, fake, waits) = retrying(replies, retried(1)).await;
    assert_eq!(answer(&provider, true).await.unwrap(), "Recovered");
    assert_eq!(fake.seen().len(), 2);
    assert_eq!(*waits.lock().unwrap(), [Duration::from_millis(700)]);
}

// Source: tests/agent/retries.rs::a_whole_completion_s_body_counts_in_the_try_a_stream_s_does_not
// The SDK's `send` reads a request it does not stream inside its try,
// whatever its status; a stream's body it reads after, where no retry
// reaches it.
#[tokio::test]
async fn a_whole_completion_s_body_counts_in_the_try_a_stream_s_does_not() {
    const JSON: &str = "application/json";
    let whole =
        [200, 400].map(|status| vec![dropped_in_its_body(status, JSON), saying("Recovered")]);
    for replies in whole {
        let (provider, fake, _) = retrying(replies, retried(1)).await;
        assert_eq!(answer(&provider, false).await.unwrap(), "Recovered");
        assert_eq!(fake.seen().len(), 2);
    }
    let dropped_before_its_first_event = stream_reply(Vec::new(), Ending::Lost);
    let streams = [
        (dropped_before_its_first_event, None),
        (dropped_in_its_body(400, JSON), None),
        (json_reply(400, "{}"), Some(400)),
    ];
    for (reply, status) in streams {
        let replies = vec![reply, saying_streamed("never")];
        let (provider, fake, _) = retrying(replies, retried(1)).await;
        let error = answer(&provider, true).await.unwrap_err();
        assert_eq!(
            (error.retryable, error.status_code),
            (status.is_none(), status)
        );
        assert_eq!(fake.seen().len(), 1);
    }
}

// Source: tests/agent/retries.rs::a_model_listing_that_is_no_page_of_models_fails
// Python reads `page.data` of what the SDK gave: a body that is no page of
// models with their ids raises, it is no empty listing, and the error does
// not repeat it.
#[tokio::test]
async fn a_model_listing_that_is_no_page_of_models_fails() {
    let page = format!("<html>{KEY} Sign in</html>");
    for body in [
        page.as_str(),
        "{}",
        r#"{"data": null}"#,
        r#"{"data": [{"object": "model"}]}"#,
        r#"{"data": [{"id": 5}]}"#,
        r#"{"data": [["gpt-4o"]]}"#,
        r#"[[["gpt-4o"]]]"#,
    ] {
        let fake = Fake::serving(vec![json_reply(200, body)]).await;
        let provider = pointed(elsewhere("local-model"), &fake);
        let error = provider.list_models().await.unwrap_err();
        assert_eq!(
            (error.retryable, error.status_code),
            (false, None),
            "{body}"
        );
        let printed = format!("{error} {error:?}");
        assert!(
            !printed.contains(KEY) && !printed.contains("Sign in"),
            "{printed}"
        );
    }
    // A page of models the server does not say is JSON is none either, as
    // Python's client hands on a text it does not read.
    let page = r#"{"object": "list", "data": [{"id": "local-model"}]}"#;
    let reply = Reply::Answer {
        status: 200,
        headers: vec![("content-type".to_owned(), "text/plain".to_owned())],
        body: Body::Whole(page.as_bytes().to_vec()),
    };
    let fake = Fake::serving(vec![reply]).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let error = provider.list_models().await.unwrap_err();
    assert_eq!((error.retryable, error.status_code), (false, None));
}
