//! What a retry reaches: an answer worth another try is tried again before
//! its body is read.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use roomkit_ai::AiProvider;
use roomkit_providers_client::Pace;

use super::support::{elsewhere, pointed};
use crate::AnthropicConfig;
use crate::replay::fake::{Body, Ending, Fake, Reply};

// Source: tests/transport.rs::retryable_headers_retry_even_when_the_body_is_unreadable
// (there the first answer declares a body it never sends, 10 bytes or past
// the bound of an answer, and the wait is taken; here its body is lost
// under way and the wait is kept)
// Python: the anthropic SDK retries before it reads an error body.
#[tokio::test]
async fn an_answer_worth_another_try_is_tried_again_before_its_body_is_read() {
    let unreadable = Reply::Answer {
        status: 503,
        headers: vec![("retry-after".to_owned(), "0.001".to_owned())],
        body: Body::Reads(vec![b"{\"type\": \"err".to_vec()], Ending::Lost),
    };
    let listing = Reply::json(200, r#"{"data": []}"#);
    let fake = Fake::serving(vec![unreadable, listing]).await;
    let config = AnthropicConfig {
        max_retries: 1,
        ..elsewhere("local-model")
    };
    let mut provider = pointed(config, &fake);
    let waits = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&waits);
    provider.client.set_pace(Pace {
        now: Arc::new(|| 1_000_000.0),
        wait: Arc::new(move |wait| {
            kept.lock().unwrap().push(wait);
            Box::pin(std::future::ready(()))
        }),
    });
    let models = provider.list_models().await.unwrap();
    assert_eq!(models.len(), 0);
    assert_eq!(fake.seen().len(), 2);
    assert_eq!(*waits.lock().unwrap(), [Duration::from_millis(1)]);
}
