//! What the replay of Python's cases cannot show: an image of an ordinary
//! size, attempts cancelled while their interactions are in flight, one
//! dropped off its runtime, a callback failing before any request, a server
//! that asks to be continued for ever, counts past any bound, what a failure
//! is raised from.

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

// An attempt dropped off the runtime it ran on, its future moved to a
// thread of no runtime, still tells its callback it is unknown: the runtime
// is the one it went in flight on.
#[test]
fn an_attempt_dropped_off_its_runtime_is_told_unknown() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let fake = runtime.block_on(Fake::serving(vec![Reply::Silent]));
    let provider = runtime.block_on(async { provider(&fake) });
    let (callback, mut heard) = sending();
    let mut generating = Box::pin(provider.generate_with_options("a fox", told(&callback, 1)));
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            while fake.seen().is_empty() {
                tokio::select! {
                    biased;
                    ended = &mut generating => panic!("a silent endpoint answered: {ended:?}"),
                    () = tokio::task::yield_now() => {}
                }
            }
        })
        .await
        .expect("the interaction reaches the fake");
    });
    std::thread::scope(|scope| {
        scope.spawn(move || drop(generating));
    });
    let statuses = runtime.block_on(async {
        let mut statuses = Vec::new();
        for _ in 0..2 {
            let attempt = tokio::time::timeout(Duration::from_secs(5), heard.recv()).await;
            statuses.push(attempt.expect("each move is told").unwrap().status);
        }
        statuses
    });
    let expected = [ImageAttemptStatus::Started, ImageAttemptStatus::Unknown];
    assert_eq!(statuses, expected);
}

// Counts past any bound a vendor would send are read without overflowing:
// a total under its image share, a thought count added at the bound.
#[tokio::test]
async fn usage_counts_past_their_bounds_are_read() {
    let usage = serde_json::json!({
        "total_input_tokens": i64::MIN,
        "input_tokens_by_modality": [
            {"modality": "image", "tokens": i64::MAX},
            {"modality": "image", "tokens": i64::MAX},
        ],
        "total_output_tokens": i64::MAX,
        "total_thought_tokens": i64::MAX,
    });
    let mut body: serde_json::Value = serde_json::from_str(&interaction("iVBORw0KGgo=")).unwrap();
    body["usage"] = usage;
    let fake = Fake::serving(vec![Reply::json(200, body.to_string())]).await;
    let images = provider(&fake)
        .generate("a fox", GenerateOptions::default())
        .await
        .unwrap();
    let usage = &images[0].usage;
    assert_eq!(usage["input_image_tokens"], i64::MAX);
    assert_eq!(usage["input_tokens"], 0);
    assert_eq!(usage["unclassified_output_tokens"], i64::MAX);
    assert_eq!(usage["output_tokens"], i64::MAX);
}

// A failed generation is raised from the provider's failure its first
// attempt met, as Python's is raised from `first.__cause__`: a host finds
// the status there.
#[tokio::test]
async fn a_failed_generation_is_raised_from_the_provider_s_failure() {
    let refused = serde_json::json!({ "error": { "code": 429, "message": "slow down", "status": "RESOURCE_EXHAUSTED" } });
    let fake = Fake::serving(vec![Reply::json(429, refused.to_string())]).await;
    let failed = provider(&fake)
        .generate("a fox", GenerateOptions::default())
        .await
        .unwrap_err();
    let source = std::error::Error::source(generation(&failed)).expect("a cause");
    let cause = source
        .downcast_ref::<roomkit_ai::ProviderError>()
        .expect("the provider's failure");
    assert_eq!(cause.status_code, Some(429));
}

// A continuation interrupted by a refused hop says what refused it, its
// status with it, as the SDK's words embed its error's.
#[tokio::test]
async fn an_interrupted_continuation_says_its_hop_s_status() {
    let first = serde_json::json!({
        "id": "int_1", "status": "requires_continuation", "continuation_token": "tok_1", "steps": [],
    });
    let refused = serde_json::json!({ "error": { "code": 429, "message": "slow down", "status": "RESOURCE_EXHAUSTED" } });
    let replies = vec![
        Reply::json(200, first.to_string()),
        Reply::json(429, refused.to_string()),
    ];
    let fake = Fake::serving(replies).await;
    let failed = provider(&fake)
        .generate("a fox", GenerateOptions::default())
        .await
        .unwrap_err();
    let said = "gemini: Automatic continuation was interrupted after 1 completed hop(s): Error \
                code: 429 - slow down. The partial interaction";
    let error = generation(&failed).attempts[0]
        .error
        .clone()
        .unwrap_or_default();
    assert!(error.starts_with(said), "{error}");
}

// An attempt that went in flight on a runtime since shut down, then dropped
// inside another runtime, is told unknown on the runtime it is dropped on.
#[test]
fn an_attempt_dropped_after_its_runtime_ended_is_told_unknown() {
    let serving = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let fake = serving.block_on(Fake::serving(vec![Reply::Silent]));
    let polling = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let provider = polling.block_on(async { provider(&fake) });
    let (callback, mut heard) = sending();
    let mut generating = Box::pin(provider.generate_with_options("a fox", told(&callback, 1)));
    polling.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            while fake.seen().is_empty() {
                tokio::select! {
                    biased;
                    ended = &mut generating => panic!("a silent endpoint answered: {ended:?}"),
                    () = tokio::task::yield_now() => {}
                }
            }
        })
        .await
        .expect("the interaction reaches the fake");
    });
    drop(polling);
    let statuses = serving.block_on(async move {
        drop(generating);
        let mut statuses = Vec::new();
        for _ in 0..2 {
            let attempt = tokio::time::timeout(Duration::from_secs(5), heard.recv()).await;
            statuses.push(attempt.expect("each move is told").unwrap().status);
        }
        statuses
    });
    let expected = [ImageAttemptStatus::Started, ImageAttemptStatus::Unknown];
    assert_eq!(statuses, expected);
}
