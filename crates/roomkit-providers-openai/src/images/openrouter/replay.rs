//! The OpenRouter image provider held to the cases captured on RoomKit's
//! Python `OpenRouterImageProvider`
//! (`tests/fixtures/python_openrouter_image_cases.json`, written by
//! `scripts/generate_openrouter_image_cases.py` from
//! `scripts/openrouter_image_cases.py`): the same requests sent, the same
//! images or failure given.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_client::Client;
use roomkit_providers_client::harness::images::{
    IMAGE_GENERATE, IMAGE_WITH_OPTIONS, replayed_or_named,
};
use roomkit_providers_client::harness::named::{Does, Instead, Named, Says, fails};
use roomkit_providers_client::harness::{Fixture, Replays};
use serde_json::Value;

use super::{OpenRouterImage, OpenRouterImageConfig};

const FIXTURE: &str = include_str!("../../../tests/fixtures/python_openrouter_image_cases.json");

impl Replays for OpenRouterImage {
    const KEY_HEADER: &'static str = "authorization";

    fn built(config: &Value) -> Result<Self, ProviderError> {
        let config: OpenRouterImageConfig = serde_json::from_value(config.clone()).unwrap();
        Self::new(config)
    }

    fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    fn point_at(&mut self, fake: SocketAddr) {
        Self::point_at(self, fake);
    }
}

/// The cases answered otherwise than Python.
const NAMED: Named = Named {
    questioned_otherwise: &[],
    only_python: &[(
        "never_connected",
        "Python's transport raises a connection error before any connection, which reads as \
         a request that never left: it may be tried again. The fake accepts every connection, \
         so the one it refuses is made then closed, which the client reads as lost once sent: \
         final. A connection that cannot be made may be tried again \
         (a_connection_never_made_may_be_tried_again).",
        &["the failure's retryable differs"],
        fails(None, false, "the connection to the provider failed"),
    )],
    by_its_own_rule: &[(
        "a_refusal_of_a_long_body",
        "The shared client bounds every failure's text to 500 characters, the provider's own \
         words with what the server said: Python repeats the server's first 500 characters \
         after its own words.",
        &["the failure says"],
        Instead {
            requests: 1,
            does: Does::Fails(
                Some(400),
                false,
                None,
                Says::Holding("OpenRouter images API returned 400: xxxxxxxxxx"),
            ),
        },
    )],
};

#[tokio::test]
async fn every_generation_replays_as_python() {
    let fixture = Fixture::read(FIXTURE);
    let (as_python, named) =
        replayed_or_named::<OpenRouterImage>(&fixture, &NAMED, IMAGE_GENERATE).await;
    assert_eq!((as_python, named), (37, 2));
}

#[tokio::test]
async fn every_generation_with_its_controls_replays_as_python() {
    let fixture = Fixture::read(FIXTURE);
    let call = IMAGE_WITH_OPTIONS;
    let (as_python, named) = replayed_or_named::<OpenRouterImage>(&fixture, &NAMED, call).await;
    assert_eq!((as_python, named), (4, 0));
}

// The replay compares what it says it compares: each wrong refused.
#[tokio::test]
async fn the_replay_refuses_what_python_did_not_do() {
    use roomkit_providers_client::harness::images::{IMAGES, REFUSAL, refuses};
    use roomkit_providers_client::harness::self_test::REQUEST;
    let fixture = Fixture::read(FIXTURE);
    refuses::<OpenRouterImage>(&fixture, "one_image", &REQUEST).await;
    refuses::<OpenRouterImage>(&fixture, "two_images", &IMAGES).await;
    refuses::<OpenRouterImage>(&fixture, "too_many_images", &REFUSAL).await;
}

// Python's catalogue of OpenRouter's image models, read whole, and the
// configured model found in it.
#[test]
fn the_image_catalogue_reads_whole() {
    use roomkit_ai::image::ImageProvider;
    let written: Value =
        serde_json::from_str(include_str!("../../../catalog/openrouter_images.json")).unwrap();
    let models = OpenRouterImage::available_models();
    assert_eq!(models.len(), written["models"].as_array().unwrap().len());
    let config = OpenRouterImageConfig::new("placeholder-not-a-key", "openai/gpt-image-2");
    let provider = OpenRouterImage::new(config).unwrap();
    assert_eq!(provider.catalog_entry().unwrap().id(), "openai/gpt-image-2");
}
