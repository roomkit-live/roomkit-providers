//! A scripted image provider, for the tests of the image crossing: it
//! speaks no wire and belongs to no vendor.
//!
//! It is an image vendor of no vendor ([`IMAGE_VENDOR`]), so that what every
//! image vendor's entries do is tested with none: a script is its
//! configuration. It tells its callback the attempts the script lists, then
//! gives the script's images, or panics as the script asks.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_ai::image::{
    GenerateOptions, GenerateWithOptionsOptions, ImageAttempt, ImageAttemptStatus, ImageError,
    ImageProvider, ImageResult, notify_image_progress,
};
use roomkit_providers_client::ReplayWaits;
use serde::Deserialize;

use super::image_vendor::{ImageEntries, ImageVendor};

/// What a scripted image provider does, as its test writes it.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ImageScript {
    /// The name it gives itself: the one its vendor states unless given.
    name: Option<String>,
    /// The key it was built with, which crosses apart.
    api_key: Option<String>,
    /// The moves of its one attempt it tells the callback, in order.
    tells: Vec<ImageAttemptStatus>,
    /// The images it gives.
    images: Vec<ImageResult>,
    /// Whether the generation panics once the attempts are told.
    panics: bool,
}

/// The scripted image provider.
#[derive(Debug)]
pub(crate) struct ScriptedImages {
    script: ImageScript,
}

impl ImageProvider for ScriptedImages {
    fn name(&self) -> &str {
        self.script.name.as_deref().unwrap_or(RECORDED_AS)
    }

    fn model_name(&self) -> &'static str {
        "scripted-image"
    }

    fn generate<'a>(
        &'a self,
        prompt: &'a str,
        options: GenerateOptions,
    ) -> impl Future<Output = Result<Vec<ImageResult>, ImageError>> + Send + 'a {
        let options = GenerateWithOptionsOptions {
            size: options.size,
            n: options.n,
            reference_images: options.reference_images,
            ..GenerateWithOptionsOptions::default()
        };
        self.generate_with_options(prompt, options)
    }

    #[expect(
        clippy::manual_async_fn,
        reason = "the trait's future, of the prompt's and the provider's lifetime"
    )]
    fn generate_with_options<'a>(
        &'a self,
        _prompt: &'a str,
        options: GenerateWithOptionsOptions,
    ) -> impl Future<Output = Result<Vec<ImageResult>, ImageError>> + Send + 'a {
        async move {
            let mut attempt = ImageAttempt::default();
            for status in &self.script.tells {
                attempt.status = *status;
                let told = notify_image_progress(options.on_progress.as_ref(), &attempt, LABEL);
                told.await?;
            }
            assert!(!self.script.panics, "the script asks a panic");
            Ok(self.script.images.clone())
        }
    }
}

/// The name a scripted image provider is recorded as, and its label.
const RECORDED_AS: &str = "ScriptedImageProvider";
const LABEL: &str = "scripted";

/// The scripted image provider as a vendor: its script is its
/// configuration.
#[derive(Debug)]
pub(crate) struct ScriptedImageVendor;

impl ImageVendor for ScriptedImageVendor {
    type Config = ImageScript;
    type Provider = ScriptedImages;

    const NAME: &'static str = "scripted";
    const RECORDED_AS: &'static str = RECORDED_AS;
    const LABEL: &'static str = LABEL;

    fn keyed(mut config: ImageScript, key: Option<String>) -> ImageScript {
        config.api_key = key;
        config
    }

    fn built(config: ImageScript) -> Result<ScriptedImages, ProviderError> {
        Ok(ScriptedImages { script: config })
    }

    fn replaying(
        _provider: ScriptedImages,
        _fake: SocketAddr,
        _clock: f64,
    ) -> Result<(ScriptedImages, ReplayWaits), ProviderError> {
        Err(ProviderError::from(
            "a scripted provider has no wire to replay".to_owned(),
        ))
    }
}

/// The entries of the scripted image provider, beside the vendors'.
pub(crate) static IMAGE_VENDOR: ImageEntries = ImageEntries::of::<ScriptedImageVendor>();
