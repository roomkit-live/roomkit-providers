//! What the replay of Python's cases cannot show: a connection that cannot
//! be made, an image of an ordinary size, a generation cancelled while its
//! request is in flight.

use std::sync::Arc;
use std::time::Duration;

use roomkit_ai::image::{
    GenerateOptions, GenerateWithOptionsOptions, ImageAttempt, ImageAttemptStatus, ImageError,
    ImageGenerationError, ImageProgressCallback, ImageProvider,
};
use roomkit_providers_client::harness::fake::{Ending, Fake, Reply};
use tokio::sync::mpsc;

use super::{OpenAiImage, OpenAiImageConfig};

fn provider(base_url: String) -> OpenAiImage {
    let mut config = OpenAiImageConfig::new("placeholder-not-a-key", "gpt-image-2");
    config.base_url = Some(base_url);
    OpenAiImage::new(config).unwrap()
}

/// A callback that sends each attempt it is told of.
fn sending() -> (ImageProgressCallback, mpsc::UnboundedReceiver<ImageAttempt>) {
    let (told, heard) = mpsc::unbounded_channel();
    let callback: ImageProgressCallback = Arc::new(move |attempt| {
        let told = told.clone();
        Box::pin(async move {
            told.send(attempt).ok();
            Ok(())
        })
    });
    (callback, heard)
}

fn told(callback: &ImageProgressCallback) -> GenerateWithOptionsOptions {
    GenerateWithOptionsOptions {
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

// A connection that cannot be made: the request never left, so the vendor
// ran nothing, and the attempt is failed, as Python's `never_sent` reads
// it (RFC §25.2).
#[tokio::test]
async fn a_connection_never_made_fails_its_attempt() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed = listener.local_addr().unwrap();
    drop(listener);
    let provider = provider(format!("http://{closed}/v1"));
    let (callback, mut heard) = sending();
    let failed = provider
        .generate_with_options("a fox", told(&callback))
        .await
        .unwrap_err();
    assert_eq!(
        generation(&failed).attempts[0].status,
        ImageAttemptStatus::Failed
    );
    let statuses = [
        heard.recv().await.unwrap().status,
        heard.recv().await.unwrap().status,
    ];
    assert_eq!(
        statuses,
        [ImageAttemptStatus::Started, ImageAttemptStatus::Failed]
    );
}

/// The base64 of `bytes` zero bytes.
fn zeros(bytes: usize) -> String {
    "A".repeat(bytes.div_ceil(3) * 4)
}

// An image of an ordinary size is read whole: a few megabytes of base64 in
// an answer, and in an event of a stream, past the bounds of a model's
// answer.
#[tokio::test]
async fn an_image_of_megabytes_is_read() {
    let whole =
        serde_json::json!({ "data": [{ "b64_json": zeros(5 << 20) }], "output_format": "png" });
    let completed = serde_json::json!({
        "type": "image_generation.completed", "b64_json": zeros(3 << 19), "output_format": "png",
    });
    let event = format!("data: {completed}\n\n").into_bytes();
    let fake = Fake::serving(vec![
        Reply::json(200, whole.to_string()),
        Reply::stream(vec![event], Ending::Ends),
    ])
    .await;
    let provider = provider(format!("http://{}/v1", fake.address));
    let images = provider
        .generate("a fox", GenerateOptions::default())
        .await
        .unwrap();
    assert_eq!(images.len(), 1);
    let (callback, _heard) = sending();
    let options = GenerateWithOptionsOptions {
        options: Some(roomkit_ai::image::ImageOptions {
            partial_images: Some(1),
            ..Default::default()
        }),
        ..told(&callback)
    };
    let streamed = provider
        .generate_with_options("a fox", options)
        .await
        .unwrap();
    assert_eq!(streamed.len(), 1);
}

// A generation dropped while its request is in flight tells its callback
// the attempt unknown: the vendor may still bill it (RFC §25.2), as
// Python's provider tells it from its `CancelledError`.
#[tokio::test]
async fn a_cancelled_generation_tells_its_attempt_unknown() {
    let fake = Fake::serving(vec![Reply::Silent]).await;
    let provider = provider(format!("http://{}/v1", fake.address));
    let (callback, mut heard) = sending();
    let generating = provider.generate_with_options("a fox", told(&callback));
    let mut generating = Box::pin(generating);
    tokio::select! {
        biased;
        ended = &mut generating => panic!("a silent endpoint answered: {ended:?}"),
        started = heard.recv() => assert_eq!(started.unwrap().status, ImageAttemptStatus::Started),
    }
    // The request is sent: once the fake read it, the generation is
    // dropped as a cancelled task drops it.
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
    .expect("the request reaches the fake");
    drop(generating);
    let told = tokio::time::timeout(Duration::from_secs(5), heard.recv()).await;
    let told = told.expect("the cancelled attempt is told").unwrap();
    assert_eq!(told.status, ImageAttemptStatus::Unknown);
    let said = "Cancelled locally; the provider may still bill this request";
    assert_eq!(told.error.as_deref(), Some(said));
}

// A callback that fails when told the attempt started fails the generation
// before any request, as Python's: nothing is in flight, so nothing is told
// unknown.
#[tokio::test]
async fn a_callback_failing_when_told_started_sends_nothing() {
    let fake = Fake::serving(Vec::new()).await;
    let provider = provider(format!("http://{}/v1", fake.address));
    let (tell, mut heard) = mpsc::unbounded_channel();
    let callback: ImageProgressCallback = Arc::new(move |attempt: ImageAttempt| {
        let tell = tell.clone();
        Box::pin(async move {
            tell.send(attempt.status).ok();
            Err("storage unavailable".into())
        })
    });
    let failed = provider
        .generate_with_options("a fox", told(&callback))
        .await;
    assert!(failed.is_err());
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

// An error a stream sends leaves through the provider's exit: what it
// repeats of the key is cleaned out, of the failure and of its attempt.
#[tokio::test]
async fn a_stream_error_repeats_no_key() {
    let error = serde_json::json!({ "error": { "message": "refused for placeholder-not-a-key" } });
    let event = format!("data: {error}\n\n").into_bytes();
    let fake = Fake::serving(vec![Reply::stream(vec![event], Ending::Ends)]).await;
    let provider = provider(format!("http://{}/v1", fake.address));
    let (callback, _heard) = sending();
    let options = GenerateWithOptionsOptions {
        options: Some(roomkit_ai::image::ImageOptions {
            partial_images: Some(1),
            ..Default::default()
        }),
        ..told(&callback)
    };
    let failed = provider
        .generate_with_options("a fox", options)
        .await
        .unwrap_err();
    let generation = generation(&failed);
    assert!(
        !generation.message.contains("placeholder-not-a-key"),
        "{}",
        generation.message
    );
    let attempt_error = generation.attempts[0].error.as_deref().unwrap_or_default();
    assert!(
        !attempt_error.contains("placeholder-not-a-key"),
        "{attempt_error}"
    );
}
