//! The image provider held to the cases captured on RoomKit's Python
//! `GeminiImageProvider` (`tests/fixtures/python_image_cases.json`, written
//! by `scripts/generate_gemini_image_cases.py` from
//! `scripts/gemini_image_cases.py`): the same interactions sent, the same
//! images or failure given, the same attempts told.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_client::Client;
use roomkit_providers_client::harness::images::{
    IMAGE_GENERATE, IMAGE_WITH_OPTIONS, replayed_or_named,
};
use roomkit_providers_client::harness::named::Named;
use roomkit_providers_client::harness::{Fixture, Replays};
use serde_json::Value;

use super::{GeminiImage, GeminiImageConfig};
use crate::endpoint::KEY_HEADER;

const FIXTURE: &str = include_str!("../../tests/fixtures/python_image_cases.json");

impl Replays for GeminiImage {
    const KEY_HEADER: &'static str = KEY_HEADER;

    fn built(config: &Value) -> Result<Self, ProviderError> {
        let config: GeminiImageConfig = serde_json::from_value(config.clone()).unwrap();
        Self::new(config)
    }

    fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    fn point_at(&mut self, fake: SocketAddr) {
        Self::point_at(self, fake);
    }
}

/// The cases answered otherwise than Python: none.
const NAMED: Named = Named {
    questioned_otherwise: &[],
    only_python: &[],
    by_its_own_rule: &[],
};

#[tokio::test]
async fn every_generation_replays_as_python() {
    let fixture = Fixture::read(FIXTURE);
    let (as_python, named) =
        replayed_or_named::<GeminiImage>(&fixture, &NAMED, IMAGE_GENERATE).await;
    assert_eq!((as_python, named), (38, 0));
}

#[tokio::test]
async fn every_generation_with_its_controls_replays_as_python() {
    let fixture = Fixture::read(FIXTURE);
    let call = IMAGE_WITH_OPTIONS;
    let (as_python, named) = replayed_or_named::<GeminiImage>(&fixture, &NAMED, call).await;
    assert_eq!((as_python, named), (19, 0));
}

// The replay compares what it says it compares: each wrong refused.
#[tokio::test]
async fn the_replay_refuses_what_python_did_not_do() {
    use roomkit_providers_client::harness::images::{GENERATION, IMAGES, REFUSAL, TOLD, refuses};
    use roomkit_providers_client::harness::self_test::REQUEST;
    let fixture = Fixture::read(FIXTURE);
    refuses::<GeminiImage>(&fixture, "one_image", &REQUEST).await;
    // A hop is compared as the first: another interaction continued.
    let another_owner: roomkit_providers_client::harness::self_test::Wrong = (
        "another interaction continued",
        "request 1: its body",
        |r| {
            let body = &mut r.seen[1].body;
            let at = body
                .windows(5)
                .position(|window| window == b"int_1")
                .unwrap();
            body[at + 4] = b'9';
        },
    );
    refuses::<GeminiImage>(&fixture, "a_generation_in_two_hops", &[another_owner]).await;
    refuses::<GeminiImage>(&fixture, "two_images", &IMAGES).await;
    refuses::<GeminiImage>(&fixture, "two_images", &TOLD).await;
    // The attempts made at once are compared each in its own order, not in
    // the order their moves came.
    roomkit_providers_client::harness::images::allows::<GeminiImage>(
        &fixture,
        "two_images",
        &roomkit_providers_client::harness::images::INTERLEAVED,
    )
    .await;
    refuses::<GeminiImage>(&fixture, "refused_with_429", &GENERATION).await;
    refuses::<GeminiImage>(&fixture, "a_size_of_a_tier_the_model_lacks", &REFUSAL).await;
}

// Python's catalogue of image models, read whole: every entry states its
// image capabilities, and a model it does not list has none.
#[test]
fn the_image_catalogue_reads_whole() {
    use roomkit_ai::image::{IMAGE_GEN_CAPABILITY, ImageCatalogEntry, ImageProvider};
    let written: Value = serde_json::from_str(include_str!("../../catalog/images.json")).unwrap();
    let models = GeminiImage::available_models();
    assert_eq!(models.len(), written["models"].as_array().unwrap().len());
    for entry in &models {
        let ImageCatalogEntry::Image(image) = entry else {
            panic!("no image capabilities: {entry:?}");
        };
        let tagged = image
            .capabilities
            .iter()
            .any(|tag| tag == IMAGE_GEN_CAPABILITY);
        assert!(tagged, "{}", image.id);
    }
    let provider = GeminiImage::new(GeminiImageConfig::new("placeholder-not-a-key")).unwrap();
    let entry = provider
        .catalog_entry()
        .expect("the default model is listed");
    assert_eq!(entry.id(), "gemini-nano-banana-2.1");
    let mut config = GeminiImageConfig::new("placeholder-not-a-key");
    config.model = "gemini-image-of-tomorrow".to_owned();
    assert!(GeminiImage::new(config).unwrap().catalog_entry().is_none());
}
