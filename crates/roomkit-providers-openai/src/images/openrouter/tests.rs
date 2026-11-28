//! What the replay of Python's cases cannot show: a connection that cannot
//! be made, the first failure of several requests stopping the others, an
//! answer of the size a real image makes.

use std::time::Duration;

use roomkit_ai::image::{GenerateOptions, ImageError, ImageProvider};
use roomkit_providers_client::harness::fake::{Fake, Reply};

use super::{OpenRouterImage, OpenRouterImageConfig};

fn provider(base_url: String) -> OpenRouterImage {
    let mut config = OpenRouterImageConfig::new("placeholder-not-a-key", "openai/gpt-image-2");
    config.base_url = base_url;
    OpenRouterImage::new(config).unwrap()
}

fn failure(failed: ImageError) -> roomkit_ai::ProviderError {
    match failed {
        ImageError::Provider(error) => error,
        ImageError::Value(refused) => panic!("refused before any request: {refused}"),
    }
}

// A connection that cannot be made: the request never left, so the vendor
// ran nothing and it may be tried again, as Python's `never_sent` reads it.
#[tokio::test]
async fn a_connection_never_made_may_be_tried_again() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed = listener.local_addr().unwrap();
    drop(listener);
    let provider = provider(format!("http://{closed}/api/v1"));
    let failed = provider.generate("a fox", GenerateOptions::default()).await;
    let error = failure(failed.unwrap_err());
    assert!(error.retryable, "{error}");
    assert_eq!(error.status_code, None);
}

// The first of several requests to fail stops the others: a generation of
// two whose second is refused ends with that refusal, never waiting for the
// first, which no one would read.
#[tokio::test]
async fn the_first_failure_stops_the_other_requests() {
    let refused = serde_json::json!({ "error": { "code": 429, "message": "slow down" } });
    let fake = Fake::serving(vec![Reply::Silent, Reply::json(429, refused.to_string())]).await;
    let provider = provider(format!("http://{}/api/v1", fake.address));
    let options = GenerateOptions {
        n: 2,
        ..GenerateOptions::default()
    };
    let failed = tokio::time::timeout(Duration::from_secs(5), provider.generate("a fox", options));
    let error = failure(failed.await.expect("the refusal ends it").unwrap_err());
    assert_eq!(error.status_code, Some(429));
    assert!(error.retryable);
}

// An image of the largest sizes is megabytes of base64: an answer past the
// shared client's few megabytes is read whole.
#[tokio::test]
async fn an_answer_of_megabytes_is_read_whole() {
    let payload = "A".repeat(5 << 20);
    let drawn = serde_json::json!({ "data": [{ "b64_json": payload, "media_type": "image/png" }] });
    let fake = Fake::serving(vec![Reply::json(200, drawn.to_string())]).await;
    let provider = provider(format!("http://{}/api/v1", fake.address));
    let images = provider.generate("a fox", GenerateOptions::default()).await;
    let [image] = images.unwrap().try_into().unwrap();
    assert_eq!(image.data.len(), "data:image/png;base64,".len() + (5 << 20));
}
