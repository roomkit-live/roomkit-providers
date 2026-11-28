//! What the replay of Python's cases cannot show: an image of an ordinary
//! size, attempts cancelled while their interactions are in flight, a
//! callback failing before any request, a server that asks to be continued
//! for ever.

use std::sync::Arc;
use std::time::Duration;

use roomkit_ai::image::{
    GenerateOptions, GenerateWithOptionsOptions, ImageAttempt, ImageAttemptStatus, ImageError,
    ImageGenerationError, ImageProgressCallback, ImageProvider,
};
use roomkit_providers_client::harness::fake::{Fake, Reply};
use tokio::sync::mpsc;

use super::{GeminiImage, GeminiImageConfig, HOPS_AT_MOST};

fn provider(fake: &Fake) -> GeminiImage {
    let mut config = GeminiImageConfig::new("placeholder-not-a-key");
    config.model = "gemini-3-pro-image".to_owned();
    let mut provider = GeminiImage::new(config).unwrap();
    provider.point_at(fake.address);
    provider
}

/// A callback that sends each attempt it is told of.
fn sending() -> (ImageProgressCallback, mpsc::UnboundedReceiver<ImageAttempt>) {
    let (tell, heard) = mpsc::unbounded_channel();
    let callback: ImageProgressCallback = Arc::new(move |attempt| {
        let tell = tell.clone();
        Box::pin(async move {
            tell.send(attempt).ok();
            Ok(())
        })
    });
    (callback, heard)
}

fn told(callback: &ImageProgressCallback, n: u32) -> GenerateWithOptionsOptions {
    GenerateWithOptionsOptions {
        n,
        on_progress: Some(Arc::clone(callback)),
        ..GenerateWithOptionsOptions::default()
    }
}

/// The generation failure of `failed`.
fn generation(failed: &ImageError) -> &ImageGenerationError {
    let ImageError::Provider(error) = failed else {
        panic!("no provider's failure: {failed:?}");
    };
    let source = std::error::Error::source(error).and_then(|source| source.downcast_ref());
    source.expect("a generation failed")
}

/// An interaction holding the image `payload`, as base64.
fn interaction(payload: &str) -> String {
    let image = serde_json::json!({ "type": "image", "data": payload, "mime_type": "image/png" });
    let step = serde_json::json!({ "type": "model_output", "content": [image] });
    serde_json::json!({ "id": "int_1", "status": "completed", "steps": [step] }).to_string()
}

// An image of an ordinary size is read whole: megabytes of base64 in one
// interaction, past the bounds of a model's answer.
#[tokio::test]
async fn an_image_of_megabytes_is_read() {
    let payload = "A".repeat((5 << 20) / 3 * 4);
    let fake = Fake::serving(vec![Reply::json(200, interaction(&payload))]).await;
    let images = provider(&fake)
        .generate("a fox", GenerateOptions::default())
        .await
        .unwrap();
    assert_eq!(images.len(), 1);
}

// Attempts dropped while their interactions are in flight each tell their
// callback they are unknown: the vendor may still bill them (RFC §25.2),
// as Python's provider tells each from its `CancelledError`.
#[tokio::test]
async fn cancelled_attempts_are_each_told_unknown() {
    let fake = Fake::serving(vec![Reply::Silent, Reply::Silent]).await;
    let provider = provider(&fake);
    let (callback, mut heard) = sending();
    let mut generating = Box::pin(provider.generate_with_options("a fox", told(&callback, 2)));
    // Both interactions are sent: once the fake read them, the generation
    // is dropped as a cancelled task drops it.
    tokio::time::timeout(Duration::from_secs(5), async {
        while fake.seen().len() < 2 {
            tokio::select! {
                biased;
                ended = &mut generating => panic!("a silent endpoint answered: {ended:?}"),
                () = tokio::task::yield_now() => {}
            }
        }
    })
    .await
    .expect("both interactions reach the fake");
    drop(generating);
    let mut statuses = Vec::new();
    for _ in 0..4 {
        let attempt = tokio::time::timeout(Duration::from_secs(5), heard.recv()).await;
        let attempt = attempt.expect("each attempt is told").unwrap();
        statuses.push((attempt.index, attempt.status, attempt.error));
    }
    statuses.sort_by_key(|(index, status, _)| (*index, *status == ImageAttemptStatus::Unknown));
    let said = Some("Cancelled locally; the provider may still bill this request".to_owned());
    let expected = [
        (0, ImageAttemptStatus::Started, None),
        (0, ImageAttemptStatus::Unknown, said.clone()),
        (1, ImageAttemptStatus::Started, None),
        (1, ImageAttemptStatus::Unknown, said),
    ];
    assert_eq!(statuses, expected);
}

// A callback that fails when told an attempt started fails it before its
// interaction: nothing is in flight, so nothing is told unknown.
#[tokio::test]
async fn a_callback_failing_when_told_started_sends_nothing() {
    let fake = Fake::serving(Vec::new()).await;
    let (tell, mut heard) = mpsc::unbounded_channel();
    let callback: ImageProgressCallback = Arc::new(move |attempt: ImageAttempt| {
        let tell = tell.clone();
        Box::pin(async move {
            tell.send(attempt.status).ok();
            Err("storage unavailable".into())
        })
    });
    let failed = provider(&fake)
        .generate_with_options("a fox", told(&callback, 1))
        .await
        .unwrap_err();
    let attempts = &generation(&failed).attempts;
    assert_eq!(attempts[0].status, ImageAttemptStatus::Started);
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    assert_eq!(heard.recv().await, Some(ImageAttemptStatus::Started));
    assert!(
        heard.try_recv().is_err(),
        "an attempt told after its failed start"
    );
    assert!(fake.seen().is_empty());
}

// A server that asks for ever to be continued, a new token each hop, is
// continued a bounded number of hops: the interaction then holds no image.
#[tokio::test]
async fn an_endless_continuation_is_bounded() {
    let hop = |at: usize| {
        let body = serde_json::json!({
            "id": format!("int_{at}"),
            "status": "requires_continuation",
            "continuation_token": format!("tok_{at}"),
            "steps": [],
        });
        Reply::json(200, body.to_string())
    };
    let fake = Fake::serving((0..HOPS_AT_MOST).map(hop).collect()).await;
    let failed = provider(&fake)
        .generate("a fox", GenerateOptions::default())
        .await
        .unwrap_err();
    let said = "Gemini returned an interaction with no image; status='requires_continuation'";
    assert_eq!(generation(&failed).message, said);
    assert_eq!(fake.seen().len(), HOPS_AT_MOST);
}
