//! The image provider held to the cases captured on RoomKit's Python
//! `OpenAIImageProvider` (`tests/fixtures/python_image_cases.json`, written by
//! `scripts/generate_openai_image_cases.py`): the same requests sent, the
//! same images or failure given, the same attempts told.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_client::Client;
use roomkit_providers_client::harness::images::{
    IMAGE_GENERATE, IMAGE_WITH_OPTIONS, replayed_or_named,
};
use roomkit_providers_client::harness::named::{Named, fails};
use roomkit_providers_client::harness::{Fixture, Replays};
use serde_json::Value;

use super::{OpenAiImage, OpenAiImageConfig};

const FIXTURE: &str = include_str!("../../tests/fixtures/python_image_cases.json");

impl Replays for OpenAiImage {
    const KEY_HEADER: &'static str = "authorization";

    fn built(config: &Value) -> Result<Self, ProviderError> {
        let config: OpenAiImageConfig = serde_json::from_value(config.clone()).unwrap();
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
        "Python's transport raises a connection error before any connection, which the \
         SDK reads as a request that never left: its attempt failed. The fake accepts every \
         connection, so the one it refuses is made then closed, which the client reads as \
         lost once sent: the attempt is unknown. A connection that cannot be made is read as \
         never sent (the client's a_transport_failure_says_whether_its_request_left).",
        &["the failure's attempts differs"],
        fails(None, false, "the connection to the provider failed"),
    )],
    by_its_own_rule: &[],
};

#[tokio::test]
async fn every_generation_replays_as_python() {
    let fixture = Fixture::read(FIXTURE);
    let (as_python, named) =
        replayed_or_named::<OpenAiImage>(&fixture, &NAMED, IMAGE_GENERATE).await;
    assert_eq!((as_python, named), (23, 1));
}

#[tokio::test]
async fn every_generation_with_its_controls_replays_as_python() {
    let fixture = Fixture::read(FIXTURE);
    let call = IMAGE_WITH_OPTIONS;
    let (as_python, named) = replayed_or_named::<OpenAiImage>(&fixture, &NAMED, call).await;
    assert_eq!((as_python, named), (28, 0));
}

// The replay compares what it says it compares: each wrong refused.
#[tokio::test]
async fn the_replay_refuses_what_python_did_not_do() {
    use roomkit_providers_client::harness::images::{GENERATION, IMAGES, REFUSAL, TOLD, refuses};
    use roomkit_providers_client::harness::self_test::REQUEST;
    let fixture = Fixture::read(FIXTURE);
    refuses::<OpenAiImage>(&fixture, "one_image", &REQUEST).await;
    refuses::<OpenAiImage>(&fixture, "two_images_at_a_size", &IMAGES).await;
    refuses::<OpenAiImage>(&fixture, "previews_then_the_image", &TOLD).await;
    refuses::<OpenAiImage>(&fixture, "refused_with_400", &GENERATION).await;
    refuses::<OpenAiImage>(&fixture, "too_many_images", &REFUSAL).await;
    // A multipart body is compared part by part: another byte of a file.
    let another_file: roomkit_providers_client::harness::self_test::Wrong =
        ("another byte of a file", "its body", |r| {
            let body = &mut r.seen[0].body;
            let at = body
                .windows(4)
                .position(|window| window == b"\x89PNG")
                .unwrap();
            body[at + 1] = b'Q';
        });
    refuses::<OpenAiImage>(&fixture, "a_mask_and_a_moderation", &[another_file]).await;
}

// Python's catalogue of image models, read whole: every entry states its
// image capabilities, and the active model is found by its id or by a
// verified snapshot alias.
#[test]
fn the_image_catalogue_reads_whole() {
    use roomkit_ai::image::{IMAGE_GEN_CAPABILITY, ImageCatalogEntry, ImageProvider};
    let written: Value = serde_json::from_str(include_str!("../../catalog/images.json")).unwrap();
    let models = OpenAiImage::available_models();
    assert_eq!(models.len(), written["models"].as_array().unwrap().len());
    for entry in &models {
        let ImageCatalogEntry::Image(image) = entry else {
            panic!("no image capabilities: {entry:?}");
        };
        assert!(
            image
                .capabilities
                .iter()
                .any(|tag| tag == IMAGE_GEN_CAPABILITY),
            "{}",
            image.id
        );
    }
    let config = |model: &str| OpenAiImageConfig::new("placeholder-not-a-key", model);
    let provider = OpenAiImage::new(config("gpt-image-2-2026-04-21")).unwrap();
    let entry = provider.catalog_entry().expect("an alias of gpt-image-2");
    assert_eq!(entry.id(), "gpt-image-2");
    let provider = OpenAiImage::new(config("my-image-deployment")).unwrap();
    assert!(provider.catalog_entry().is_none());
}
