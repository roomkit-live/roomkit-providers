//! `OpenAI`'s image provider (RFC §25): Python's `OpenAIImageProvider`, a
//! generation (`images.generate`) or an edit (`images.edit`, multipart),
//! whole or as a stream of previews, on the shared client.
//!
//! A paid generation is never tried again on its own: the client makes one
//! try. Every outcome is an attempt, told to a progress callback as it
//! moves; a failure once the request started keeps the images and the
//! usage it got, marked unknown where the request was lost once sent, which
//! the vendor may have run and billed, failed where it never left or the
//! vendor answered with a status (a 500, 502 or 504 may still follow a run
//! it billed: the host decides, RFC §25.2).

mod answer;
mod config;
mod request;

use std::sync::{Arc, LazyLock};

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use roomkit_ai::ProviderError;
use roomkit_ai::image::{
    GenerateOptions, GenerateWithOptionsOptions, ImageAttempt, ImageAttemptStatus,
    ImageCatalogEntry, ImageError, ImageGenerationError, ImageProgressCallback, ImageProvider,
    ImageResult, notify_image_progress,
};
use roomkit_providers_client::endpoint::{has_user_information, joined};
use roomkit_providers_client::retry::{Past, Rule};
use roomkit_providers_client::{
    Call, Client, Exit, Failure, Settings, Timeouts, configured_header, tried_again,
};
use serde_json::Value;

use self::answer::{Event, PROVIDER, Previews};
pub use self::config::OpenAiImageConfig;
use self::request::{Asked, Checked};
use crate::endpoint::OPENAI_BASE_URL;

/// The most bytes of a whole answer read: ten images of the largest size.
const ANSWER_LIMIT: usize = 512 * 1024 * 1024;

/// The most bytes of an event of a stream read: one image of the largest
/// size.
const EVENT_LIMIT: usize = 64 * 1024 * 1024;

/// No answer is waited for again: the provider makes one try.
const RETRY: Rule = Rule {
    longest_asked: 0.0,
    past_it: Past::Final,
};

/// The catalogue of `OpenAI`'s image models, Python's
/// (`catalog/images.json`, written by `scripts/generate_openai_catalogue.py`).
static CATALOGUE: LazyLock<Vec<ImageCatalogEntry>> = LazyLock::new(|| {
    let written: Value =
        serde_json::from_str(include_str!("../../catalog/images.json")).unwrap_or_default();
    let models = written.get("models").cloned().unwrap_or_default();
    serde_json::from_value(models).unwrap_or_default()
});

/// `OpenAI`'s image provider: Python's `OpenAIImageProvider`. A request with
/// reference images is an edit, which `OpenAI` serves at another endpoint:
/// the provider absorbs it (RFC §25.4).
#[derive(Clone)]
pub struct OpenAiImage {
    config: Arc<OpenAiImageConfig>,
    generations: reqwest::Url,
    edits: reqwest::Url,
    headers: HeaderMap,
    client: Client,
    exit: Arc<Exit>,
}

impl std::fmt::Debug for OpenAiImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiImage")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl OpenAiImage {
    /// The provider for `config`.
    ///
    /// # Errors
    ///
    /// A failure before any request: a base URL that is no `http` or
    /// `https` URL, a key or a default header that can go in no header, or
    /// a client that could not be built.
    pub fn new(config: OpenAiImageConfig) -> Result<Self, ProviderError> {
        Self::built(config).map_err(|failure| Exit::unbuilt(PROVIDER, &failure))
    }

    fn built(config: OpenAiImageConfig) -> Result<Self, Failure> {
        let base_url = config.base_url.as_deref().unwrap_or(OPENAI_BASE_URL);
        let generations = joined(base_url, "/images/generations")?;
        let edits = joined(base_url, "/images/edits")?;
        let headers = headers_of(&config, &generations)?;
        let settings = Settings {
            timeouts: Timeouts::new(config.timeout, config.connect_timeout)?,
            max_retries: 0,
            retry: RETRY,
            refused: crate::failure::status,
            tried_again: tried_again!("roomkit::providers::openai"),
        };
        let mut client = Client::new(settings)?;
        // A generated image is megabytes of base64: up to ten of them in an
        // answer, one in an event of a stream.
        client.read_up_to(ANSWER_LIMIT, EVENT_LIMIT);
        let configured = config.default_headers.iter().flatten();
        let values = configured.map(|(_, value)| value.as_str());
        let exit = client.exit(PROVIDER, [config.api_key.clone()], values, &generations);
        Ok(Self {
            config: Arc::new(config),
            generations,
            edits,
            headers,
            client,
            exit: Arc::new(exit),
        })
    }

    /// Sends the requests to `address` instead, over plain HTTP, each to the
    /// path it had: a test's fake of the endpoint.
    #[cfg(test)]
    pub(crate) fn point_at(&mut self, address: std::net::SocketAddr) {
        let urls = [&mut self.generations, &mut self.edits];
        let moved = self.client.pointed_at(urls, address);
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
        let moved =
            roomkit_providers_client::replayed_at([&mut self.generations, &mut self.edits], fake);
        moved.map_err(|failure| self.exit.leave(&failure))?;
        let waits = self.client.replaying(clock);
        Ok((self, waits))
    }

    /// The call of `body`, to the endpoint `checked` asks.
    fn call<'a>(&'a self, checked: &Checked, body: &'a [u8], headers: &'a HeaderMap) -> Call<'a> {
        Call {
            url: if checked.edit {
                &self.edits
            } else {
                &self.generations
            },
            headers,
            body: Some(body),
            token: None,
        }
    }

    /// Asks the images `checked` asks, reading what comes into `attempt`.
    async fn attempted(
        &self,
        checked: &Checked,
        expected: u32,
        attempt: &mut ImageAttempt,
        callback: Option<&ImageProgressCallback>,
    ) -> Result<(), Failed> {
        let unwritten = || Failed::own("a reference image's media type holds a control character");
        let (content_type, body) = checked.body.as_ref().map_err(|_| unwritten())?;
        let mut headers = self.headers.clone();
        let content_type = HeaderValue::from_str(content_type);
        headers.insert(CONTENT_TYPE, content_type.map_err(|_| unwritten())?);
        let call = self.call(checked, body, &headers);
        let answer = if checked.stream {
            self.streamed(call, attempt, callback).await?
        } else {
            let sent = self.client.send_headed(call).await;
            let (body, _, answered) = sent.map_err(|failure| self.failed(&failure))?;
            let id = answered.get("x-request-id").and_then(|id| id.to_str().ok());
            if let Some(id) = id {
                attempt.provider_request_id = Some(id.to_owned());
            }
            serde_json::from_slice(&body).map_err(|_| Failed::own("its answer is no JSON"))?
        };
        if checked.stream {
            attempt.results.clear();
        }
        answer::read(&answer, expected, attempt).map_err(Failed::of)
    }

    /// The stream of previews `call` answers, read to its end into
    /// `attempt`, each preview told to `callback`: the answer it makes.
    async fn streamed(
        &self,
        call: Call<'_>,
        attempt: &mut ImageAttempt,
        callback: Option<&ImageProgressCallback>,
    ) -> Result<Value, Failed> {
        let mut stream = self
            .client
            .stream(call)
            .await
            .map_err(|failure| self.failed(&failure))?;
        if let Some(id) = stream.header("x-request-id") {
            attempt.provider_request_id = Some(id.to_owned());
        }
        let mut previews = Previews::default();
        while let Some(data) = stream
            .next()
            .await
            .map_err(|failure| self.failed(&failure))?
        {
            let event = previews.read(&data, attempt, callback.is_some());
            // An error the stream sends carries no status, as Python's SDK
            // raises it (`APIError`): its type or its code says none.
            let event = event.map_err(|failure| {
                let mut failed = self.failed(&failure);
                failed.error.status_code = None;
                failed
            });
            match event? {
                Event::Preview(told) => {
                    answer::preview(callback, *told).await.map_err(Failed::of)?;
                }
                Event::Done => break,
                Event::Read => {}
            }
        }
        Ok(previews.answer())
    }

    /// The client's `failure` as the attempt reads it: unknown for a
    /// request lost once sent, which the vendor may have run, else failed.
    fn failed(&self, failure: &Failure) -> Failed {
        let sent = failure
            .transport_of()
            .is_some_and(|transport| !transport.unsent);
        let status = if sent {
            ImageAttemptStatus::Unknown
        } else {
            ImageAttemptStatus::Failed
        };
        Failed {
            status,
            error: self.exit.leave(failure),
        }
    }

    /// Generates the images `options` asks of `prompt`.
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
            mask: options.mask.as_ref(),
            told: callback.is_some(),
        };
        let checked = request::checked(&self.config, &asked, self.catalog_entry().as_ref())?;
        let attempt = ImageAttempt {
            effective_options: checked.effective.clone(),
            ..ImageAttempt::default()
        };
        // The request is in flight once the attempt is told started: a
        // failure or a cancellation of that first telling sent nothing.
        notify_image_progress(callback, &attempt, PROVIDER).await?;
        let mut in_flight = InFlight::new(attempt, callback);
        let attempted = self
            .attempted(&checked, options.n, &mut in_flight.attempt, callback)
            .await;
        let mut attempt = in_flight.settled();
        if let Err(failed) = attempted {
            attempt.status = failed.status;
            attempt.error = Some(failed.error.message.clone());
            attempt.status_code = failed.error.status_code;
            notify_image_progress(callback, &attempt, PROVIDER).await?;
            let message = failed.error.message.clone();
            let generation = ImageGenerationError::new(message, PROVIDER, vec![attempt]);
            return Err(generation.with_source(failed.error).into());
        }
        attempt.status = ImageAttemptStatus::Succeeded;
        notify_image_progress(callback, &attempt, PROVIDER).await?;
        Ok(attempt.results)
    }
}

/// The attempt of a generation while its request is in flight: a
/// generation dropped then, the call cancelled, tells its callback the
/// attempt unknown, as Python's provider does from its `CancelledError`
/// (RFC §25.2): the vendor may still bill the request. The callback is
/// told in a task, which a dropped future cannot await: on the runtime it
/// is dropped on, else, dropped off any, on the one it went in flight on.
struct InFlight {
    attempt: ImageAttempt,
    callback: Option<ImageProgressCallback>,
    /// The runtime the attempt went in flight on: the one it is told on when
    /// it is dropped off any runtime.
    runtime: Option<tokio::runtime::Handle>,
}

/// What a cancelled attempt says, as Python's.
const CANCELLED: &str = "Cancelled locally; the provider may still bill this request";

impl InFlight {
    fn new(attempt: ImageAttempt, callback: Option<&ImageProgressCallback>) -> Self {
        Self {
            attempt,
            callback: callback.cloned(),
            runtime: tokio::runtime::Handle::try_current().ok(),
        }
    }

    /// The attempt, its request settled: nothing to tell on a drop.
    fn settled(mut self) -> ImageAttempt {
        self.callback = None;
        std::mem::take(&mut self.attempt)
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        let Some(callback) = self.callback.take() else {
            return;
        };
        // The runtime it is dropped on, which runs; else the one it went in
        // flight on.
        let current = tokio::runtime::Handle::try_current().ok();
        let Some(runtime) = current.or_else(|| self.runtime.take()) else {
            return;
        };
        let mut attempt = std::mem::take(&mut self.attempt);
        attempt.status = ImageAttemptStatus::Unknown;
        attempt.error = Some(CANCELLED.to_owned());
        // The callback's failure has no caller left to fail.
        drop(runtime.spawn(async move { callback(attempt).await }));
    }
}

/// The failure of an attempt: how it stands, and the failure itself.
struct Failed {
    status: ImageAttemptStatus,
    error: ProviderError,
}

impl Failed {
    /// A failure of the provider's own, which no request met.
    fn own(said: &str) -> Self {
        Self::of(answer::own_failure(said.to_owned()))
    }

    /// The attempt failed with `error`.
    fn of(error: ProviderError) -> Self {
        Self {
            status: ImageAttemptStatus::Failed,
            error,
        }
    }
}

/// The headers every request carries: the key as a bearer, unless the base
/// URL holds user information, then the configuration's, which replace the
/// provider's of the same name.
fn headers_of(config: &OpenAiImageConfig, url: &reqwest::Url) -> Result<HeaderMap, Failure> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    if !config.api_key.is_empty() && !has_user_information(url) {
        let bearer = HeaderValue::from_str(&format!("Bearer {}", config.api_key));
        let mut bearer = bearer.map_err(|_| Failure::unbuilt("its key cannot go in a header"))?;
        bearer.set_sensitive(true);
        headers.insert(AUTHORIZATION, bearer);
    }
    for (name, value) in config.default_headers.iter().flatten() {
        let no_header = || Failure::unbuilt("one of its default headers is no HTTP header");
        let (name, value) = configured_header(name, value).ok_or_else(no_header)?;
        headers.insert(name, value);
    }
    Ok(headers)
}

impl ImageProvider for OpenAiImage {
    fn name(&self) -> &'static str {
        "OpenAIImageProvider"
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
