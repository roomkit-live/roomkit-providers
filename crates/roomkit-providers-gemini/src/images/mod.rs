//! Gemini's image provider (RFC §25): Python's `GeminiImageProvider`, on
//! the Interactions API (`POST v1beta/interactions` with an image response
//! format), on the shared client.
//!
//! One interaction draws one image, so `n` images are `n` interactions,
//! made at once and each settled alone: a failure of one keeps the image
//! another drew. A paid interaction is never tried again on its own; one
//! cut at the decode limit is continued hop after hop, as Python's SDK
//! continues it. Every outcome is an attempt, told to a progress callback
//! as it moves: failed once an interaction came back or the vendor
//! answered with a status, unknown otherwise, the vendor having maybe run
//! and billed it (RFC §25.2).

mod answer;
mod config;
mod fields;
mod geometry;
mod hops;
mod in_flight;
mod repr;
mod request;

use std::sync::{Arc, LazyLock};

use futures_util::future::join_all;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use roomkit_ai::ProviderError;
use roomkit_ai::image::{
    GenerateOptions, GenerateWithOptionsOptions, ImageAttempt, ImageAttemptStatus,
    ImageCatalogEntry, ImageError, ImageGenerationError, ImageProgressCallback, ImageProvider,
    ImageResult, notify_image_progress,
};
use roomkit_providers_client::endpoint::joined;
use roomkit_providers_client::retry::{Past, Rule};
use roomkit_providers_client::{Call, Client, Exit, Failure, Settings, Timeouts, tried_again};
use serde_json::Value;

use self::answer::{PROVIDER, own_failure};
pub use self::config::GeminiImageConfig;
use self::in_flight::InFlight;
use self::request::{Asked, Checked, Continued};
use crate::endpoint::{DEVELOPER_API, KEY_HEADER};

/// The most bytes of an answer read: an interaction holds one image of the
/// largest tier, as base64, with its steps.
const ANSWER_LIMIT: usize = 128 * 1024 * 1024;

/// The most hops an interaction is continued for: a bound on a server that
/// would ask to be continued for ever. Past it, the interaction reads as
/// the last hop left it, with no image.
const HOPS_AT_MOST: usize = 64;

/// No answer is waited for again: the provider makes one try.
const RETRY: Rule = Rule {
    longest_asked: 0.0,
    past_it: Past::Final,
};

/// The catalogue of Gemini's image models, Python's (`catalog/images.json`,
/// written by `scripts/generate_gemini_catalogue.py`).
static CATALOGUE: LazyLock<Vec<ImageCatalogEntry>> = LazyLock::new(|| {
    let written: Value =
        serde_json::from_str(include_str!("../../catalog/images.json")).unwrap_or_default();
    let models = written.get("models").cloned().unwrap_or_default();
    serde_json::from_value(models).unwrap_or_default()
});

/// Gemini's image provider: Python's `GeminiImageProvider`. A request with
/// reference images edits them: they go with the prompt in the
/// interaction's input.
#[derive(Clone)]
pub struct GeminiImage {
    config: Arc<GeminiImageConfig>,
    interactions: reqwest::Url,
    headers: HeaderMap,
    client: Client,
    exit: Arc<Exit>,
}

impl std::fmt::Debug for GeminiImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiImage")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl GeminiImage {
    /// The provider for `config`.
    ///
    /// # Errors
    ///
    /// A failure before any request: a key that can go in no header, a
    /// timeout the clock cannot count, or a client that could not be built.
    pub fn new(config: GeminiImageConfig) -> Result<Self, ProviderError> {
        Self::built(config).map_err(|failure| Exit::unbuilt(PROVIDER, &failure))
    }

    fn built(config: GeminiImageConfig) -> Result<Self, Failure> {
        let interactions = joined(DEVELOPER_API, "/interactions")?;
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let key = HeaderValue::from_str(&config.api_key);
        let mut key = key.map_err(|_| Failure::unbuilt("its key cannot go in a header"))?;
        key.set_sensitive(true);
        headers.insert(HeaderName::from_static(KEY_HEADER), key);
        let settings = Settings {
            timeouts: Timeouts::new(config.timeout, config.connect_timeout)?,
            max_retries: 0,
            retry: RETRY,
            refused: crate::failure::status,
            tried_again: tried_again!("roomkit::providers::gemini"),
        };
        let mut client = Client::new(settings)?;
        client.read_up_to(ANSWER_LIMIT, ANSWER_LIMIT);
        let exit = client.exit(PROVIDER, [config.api_key.clone()], [], &interactions);
        Ok(Self {
            config: Arc::new(config),
            interactions,
            headers,
            client,
            exit: Arc::new(exit),
        })
    }

    /// Sends the requests to `address` instead, over plain HTTP: a test's
    /// fake of the endpoint.
    #[cfg(test)]
    pub(crate) fn point_at(&mut self, address: std::net::SocketAddr) {
        let moved = self.client.pointed_at([&mut self.interactions], address);
        assert!(moved, "the endpoint is pointed at the fake");
    }

    /// The provider as a replay of recorded cases drives it, outside this
    /// crate's tests: its requests go to `fake`, a local fake of its
    /// endpoint, over plain HTTP, each to the path it had; its clock is set
    /// to `clock`, in seconds since 1970. It makes one try, so it keeps no
    /// wait.
    ///
    /// # Errors
    ///
    /// A `fake` that is no loopback address: a replay sends nothing off the
    /// machine.
    #[cfg(feature = "replay")]
    #[doc(hidden)]
    pub fn replaying(
        mut self,
        fake: std::net::SocketAddr,
        clock: f64,
    ) -> Result<(Self, roomkit_providers_client::ReplayWaits), ProviderError> {
        let moved = roomkit_providers_client::replayed_at([&mut self.interactions], fake);
        moved.map_err(|failure| self.exit.leave(&failure))?;
        let waits = self.client.replaying(clock);
        Ok((self, waits))
    }

    /// One hop of an interaction: the interaction `body` asks, as it came
    /// back.
    async fn hop(&self, body: &[u8]) -> Result<Value, Failure> {
        let call = Call {
            url: &self.interactions,
            headers: &self.headers,
            body: Some(body),
            token: None,
        };
        let (answer, _) = self.client.send(call).await?;
        // Any JSON is an interaction to the SDK, which builds what its
        // models do not validate member by member.
        serde_json::from_slice(&answer).map_err(|error| Failure::malformed(&error))
    }

    /// The interaction `checked` asks, continued hop after hop until it
    /// completes, its hops as one.
    ///
    /// # Errors
    ///
    /// The failure of a hop, through the provider's exit; a hop after the
    /// first fails as an interrupted continuation, which has no status.
    async fn created(&self, checked: &Checked) -> Result<Value, ProviderError> {
        let first = self.hop(&checked.body(None)).await;
        let mut hops = vec![first.map_err(|failure| self.exit.leave(&failure))?];
        let mut sent: Option<String> = None;
        while hops.len() < HOPS_AT_MOST {
            let last = hops.last().unwrap_or(&Value::Null);
            let Some(token) = hops::next_token(last, sent.as_deref()) else {
                break;
            };
            let continued = Continued {
                token,
                owner: hops::owner(last),
            };
            let token = token.to_owned();
            match self.hop(&checked.body(Some(&continued))).await {
                Ok(hop) => hops.push(hop),
                Err(failure) => {
                    let failed = self.exit.leave(&failure);
                    let said = hops::interrupted(hops.len(), &failed);
                    return Err(own_failure(said));
                }
            }
            sent = Some(token);
        }
        Ok(hops::merged(hops))
    }

    /// The attempt at `index`: told started, then its interaction asked and
    /// read, then told how it ended (Python's `_attempt`).
    ///
    /// # Errors
    ///
    /// The attempt's failure, the attempt in it; or the callback's.
    async fn attempt(
        &self,
        checked: &Checked,
        index: u32,
        callback: Option<&ImageProgressCallback>,
    ) -> Result<ImageAttempt, Box<AttemptFailed>> {
        let attempt = ImageAttempt {
            index,
            effective_options: checked.effective.clone(),
            ..ImageAttempt::default()
        };
        // The interaction is in flight once the attempt is told started: a
        // failure or a cancellation of that first telling sent nothing.
        notify_image_progress(callback, &attempt, PROVIDER)
            .await
            .map_err(AttemptFailed::told)?;
        let in_flight = InFlight::new(attempt, callback);
        let created = self.created(checked).await;
        let mut attempt = in_flight.settled();
        let failed = match created {
            // An interaction that came back fails its attempt: the vendor
            // ran it.
            // A `null` the SDK reads as no interaction at all.
            Ok(interaction) => answer::read(&interaction, &mut attempt)
                .err()
                .map(|error| (!interaction.is_null(), error)),
            Err(error) => Some((false, error)),
        };
        if let Some((came, error)) = &failed {
            let stated = error.status_code.filter(|status| *status != 0);
            attempt.status = if *came || stated.is_some() {
                ImageAttemptStatus::Failed
            } else {
                ImageAttemptStatus::Unknown
            };
            attempt.error = Some(error.to_string());
            attempt.status_code = error.status_code;
        } else {
            attempt.status = ImageAttemptStatus::Succeeded;
        }
        notify_image_progress(callback, &attempt, PROVIDER)
            .await
            .map_err(AttemptFailed::told)?;
        let Some((_, error)) = failed else {
            return Ok(attempt);
        };
        let generation = ImageGenerationError::new(error.to_string(), PROVIDER, vec![attempt]);
        Err(Box::new(AttemptFailed {
            generation: generation.with_source(error.clone()),
            cause: Some(error),
        }))
    }

    /// Generates the images `options` asks of `prompt`: `n` attempts at
    /// once, each settled alone; the first failure fails the generation,
    /// holding every attempt and the images of those that drew one.
    async fn generated(
        &self,
        prompt: &str,
        options: GenerateWithOptionsOptions,
    ) -> Result<Vec<ImageResult>, ImageError> {
        let callback = options.on_progress.as_ref();
        let asked = Asked {
            prompt,
            size: options.size.as_deref(),
            n: options.n,
            references: &options.reference_images,
            options: options.options.as_ref(),
            masked: options.mask.is_some(),
        };
        let checked = request::checked(&self.config, &asked, self.catalog_entry().as_ref())?;
        let attempts = (0..options.n).map(|index| self.attempt(&checked, index, callback));
        let mut attempts_made = Vec::new();
        let mut failures = Vec::new();
        for outcome in join_all(attempts).await {
            match outcome {
                Ok(attempt) => attempts_made.push(attempt),
                Err(failure) => {
                    attempts_made.extend(failure.generation.attempts.iter().cloned());
                    failures.push(failure);
                }
            }
        }
        if let Some(first) = failures.into_iter().next() {
            let message = first.generation.to_string();
            let generation = ImageGenerationError::new(message, PROVIDER, attempts_made);
            // Raised from what the first failure was raised from, as
            // Python's `from first.__cause__ or first`.
            let generation = match first.cause {
                Some(cause) => generation.with_source(cause),
                None => generation.with_source(first.generation),
            };
            return Err(generation.into());
        }
        Ok(attempts_made
            .into_iter()
            .flat_map(|attempt| attempt.results)
            .collect())
    }
}

/// An attempt that failed: its failure, holding the attempt, and the
/// provider's failure it was raised from, where the callback did not fail.
struct AttemptFailed {
    generation: ImageGenerationError,
    cause: Option<ProviderError>,
}

impl AttemptFailed {
    /// The callback's failure, which holds the attempt it was told of.
    fn told(generation: ImageGenerationError) -> Box<Self> {
        Box::new(Self {
            generation,
            cause: None,
        })
    }
}

impl ImageProvider for GeminiImage {
    fn name(&self) -> &'static str {
        "GeminiImageProvider"
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    fn supports_editing(&self) -> bool {
        true
    }

    fn available_models() -> Vec<ImageCatalogEntry> {
        CATALOGUE.clone()
    }

    fn generate<'a>(
        &'a self,
        prompt: &'a str,
        options: GenerateOptions,
    ) -> impl Future<Output = Result<Vec<ImageResult>, ImageError>> + Send + 'a {
        let GenerateOptions {
            size,
            n,
            reference_images,
        } = options;
        let options = GenerateWithOptionsOptions {
            size,
            n,
            reference_images,
            ..GenerateWithOptionsOptions::default()
        };
        async move { self.generated(prompt, options).await }
    }

    #[expect(
        clippy::manual_async_fn,
        reason = "the trait's future, of the prompt's and the provider's lifetime"
    )]
    fn generate_with_options<'a>(
        &'a self,
        prompt: &'a str,
        options: GenerateWithOptionsOptions,
    ) -> impl Future<Output = Result<Vec<ImageResult>, ImageError>> + Send + 'a {
        async move { self.generated(prompt, options).await }
    }
}

#[cfg(test)]
mod replay;
#[cfg(test)]
mod tests;
