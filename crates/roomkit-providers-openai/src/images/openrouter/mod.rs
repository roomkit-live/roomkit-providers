//! OpenRouter's image provider (RFC §25): Python's
//! `OpenRouterImageProvider`, on OpenRouter's Image API (`POST
//! /api/v1/images`), its own endpoint and request shape, on the shared
//! client.
//!
//! One request draws one image of the routed model, so `n` images are `n`
//! requests made at once, at most ten: the first failure stops the others,
//! as Python's task group cancels them. A paid generation is never tried
//! again on its own. Each image carries its own request's usage and the
//! amount OpenRouter billed for it.

mod config;

use std::sync::{Arc, LazyLock};

use futures_util::future::try_join_all;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use roomkit::model::Metadata;
use roomkit_ai::image::{
    GenerateOptions, ImageCatalogEntry, ImageError, ImageProvider, ImageRequestError, ImageResult,
    PAID_GENERATION_RETRYABLE, ParseDataUriOptions, PayloadMimeTypeOptions, parse_data_uri,
    parse_size, payload_mime_type, to_data_uri,
};
use roomkit_ai::{AiImagePart, ProviderError, ProviderErrorOptions};
use roomkit_providers_client::endpoint::joined;
use roomkit_providers_client::failure::{MESSAGE_LIMIT, own, told};
use roomkit_providers_client::retry::{Past, Rule};
use roomkit_providers_client::{
    Call, Client, Exit, Failure, Settings, Timeouts, configured_header, tried_again,
};
use serde_json::{Map, Value, json};

pub use self::config::OpenRouterImageConfig;

/// The name of the provider's failures.
const PROVIDER: &str = "openrouter";

/// The most images a request asks: the largest batch of the routed lineup.
/// Each is a request made at once, and billed.
const MOST_IMAGES: u32 = 10;

/// The most bytes of an answer read: one image of the largest size, as
/// base64.
const ANSWER_LIMIT: usize = 128 * 1024 * 1024;

/// No answer is waited for again: the provider makes one try.
const RETRY: Rule = Rule {
    longest_asked: 0.0,
    past_it: Past::Final,
};

/// The catalogue of OpenRouter's image models, Python's
/// (`catalog/openrouter_images.json`, written by
/// `scripts/generate_openai_catalogue.py`).
static CATALOGUE: LazyLock<Vec<ImageCatalogEntry>> = LazyLock::new(|| {
    let written: Value =
        serde_json::from_str(include_str!("../../../catalog/openrouter_images.json"))
            .unwrap_or_default();
    let models = written.get("models").cloned().unwrap_or_default();
    serde_json::from_value(models).unwrap_or_default()
});

/// OpenRouter's image provider: Python's `OpenRouterImageProvider`. A
/// request with reference images edits them: they ride the same request,
/// as `input_references`.
#[derive(Clone)]
pub struct OpenRouterImage {
    config: Arc<OpenRouterImageConfig>,
    images: reqwest::Url,
    headers: HeaderMap,
    client: Client,
    exit: Arc<Exit>,
}

impl std::fmt::Debug for OpenRouterImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenRouterImage")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl OpenRouterImage {
    /// The provider for `config`.
    ///
    /// # Errors
    ///
    /// A failure before any request: a base URL that is no `http` or
    /// `https` URL, a key or a header that can go in no header, or a client
    /// that could not be built.
    pub fn new(config: OpenRouterImageConfig) -> Result<Self, ProviderError> {
        Self::built(config).map_err(|failure| Exit::unbuilt(PROVIDER, &failure))
    }

    fn built(config: OpenRouterImageConfig) -> Result<Self, Failure> {
        let images = joined(config.base_url.trim_end_matches('/'), "/images")?;
        let headers = headers_of(&config)?;
        let settings = Settings {
            timeouts: Timeouts::new(config.timeout, config.connect_timeout)?,
            max_retries: 0,
            retry: RETRY,
            refused,
            tried_again: tried_again!("roomkit::providers::openrouter"),
        };
        let mut client = Client::new(settings)?;
        client.read_up_to(ANSWER_LIMIT, ANSWER_LIMIT);
        let configured = config.default_headers.iter().flatten();
        let values = configured.map(|(_, value)| value.as_str());
        let exit = client.exit(PROVIDER, [config.api_key.clone()], values, &images);
        Ok(Self {
            config: Arc::new(config),
            images,
            headers,
            client,
            exit: Arc::new(exit),
        })
    }

    /// Sends the requests to `address` instead, over plain HTTP: a test's
    /// fake of the endpoint.
    #[cfg(test)]
    pub(crate) fn point_at(&mut self, address: std::net::SocketAddr) {
        let moved = self.client.pointed_at([&mut self.images], address);
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
        let moved = roomkit_providers_client::replayed_at([&mut self.images], fake);
        moved.map_err(|failure| self.exit.leave(&failure))?;
        let waits = self.client.replaying(clock);
        Ok((self, waits))
    }

    /// One request of `body`: the answer it came back with, an object.
    async fn created(&self, body: &[u8]) -> Result<Map<String, Value>, ProviderError> {
        let call = Call {
            url: &self.images,
            headers: &self.headers,
            body: Some(body),
            token: None,
        };
        let (answer, _) = self
            .client
            .send(call)
            .await
            .map_err(|failure| self.failed(&failure))?;
        let answer: Value = serde_json::from_slice(&answer)
            .map_err(|error| self.exit.leave(&Failure::malformed(&error)))?;
        let Value::Object(payload) = answer else {
            return Err(own_failure(
                "OpenRouter images API returned a non-object payload",
            ));
        };
        Ok(payload)
    }

    /// The client's `failure` as the provider's: a request lost in transport
    /// is tried again only where it never left, the vendor having run
    /// nothing (Python's `never_sent`).
    fn failed(&self, failure: &Failure) -> ProviderError {
        let mut error = self.exit.leave(failure);
        if let Some(transport) = failure.transport_of() {
            error.retryable = transport.unsent;
        }
        error
    }

    /// Generates the `n` images `options` asks of `prompt`.
    async fn generated(
        &self,
        prompt: &str,
        options: GenerateOptions,
    ) -> Result<Vec<ImageResult>, ImageError> {
        let GenerateOptions {
            size,
            n,
            reference_images,
        } = options;
        if !(1..=MOST_IMAGES).contains(&n) {
            let said = format!("n must be between 1 and {MOST_IMAGES}, got {n}");
            return Err(ImageRequestError::new(said).into());
        }
        let body = body_of(&self.config, prompt, size.as_deref(), &reference_images)?;
        // A JSON value always writes.
        let body = serde_json::to_vec(&body).unwrap_or_default();
        let requests = (0..n).map(|_| self.created(&body));
        let payloads = try_join_all(requests).await?;
        let images = payloads.iter().map(result_of).collect::<Result<_, _>>()?;
        Ok(images)
    }
}

/// The headers every request carries: the key as a bearer, OpenRouter's
/// attribution, then the configuration's, which replace the provider's of
/// the same name.
fn headers_of(config: &OpenRouterImageConfig) -> Result<HeaderMap, Failure> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let no_header = || Failure::unbuilt("its key cannot go in a header");
    let bearer = HeaderValue::from_str(&format!("Bearer {}", config.api_key));
    let mut bearer = bearer.map_err(|_| no_header())?;
    bearer.set_sensitive(true);
    headers.insert(AUTHORIZATION, bearer);
    let attribution = [
        ("http-referer", &config.site_url),
        ("x-title", &config.app_name),
    ];
    for (name, value) in attribution {
        let Some(value) = value.as_deref().filter(|value| !value.is_empty()) else {
            continue;
        };
        let no_header = || Failure::unbuilt("its attribution cannot go in a header");
        let value = HeaderValue::from_str(value).map_err(|_| no_header())?;
        headers.insert(HeaderName::from_static(name), value);
    }
    for (name, value) in config.default_headers.iter().flatten() {
        let no_header = || Failure::unbuilt("one of its default headers is no HTTP header");
        let (name, value) = configured_header(name, value).ok_or_else(no_header)?;
        headers.insert(name, value);
    }
    Ok(headers)
}

/// The failure of a request OpenRouter refused, in Python's words: its
/// status, and the first 500 characters of what it answered. Tried again
/// only where the vendor ran nothing (`PAID_GENERATION_RETRYABLE`).
fn refused(status: u16, body: &[u8]) -> Failure {
    let answered = String::from_utf8_lossy(body);
    let said = vec![
        own(format!("OpenRouter images API returned {status}: ")),
        told(answered.into_owned(), MESSAGE_LIMIT),
    ];
    let retryable = PAID_GENERATION_RETRYABLE.contains(&status);
    Failure::said(said, retryable, Some(status))
}

/// A failure of the provider's own, in its words: final.
fn own_failure(message: &str) -> ProviderError {
    let options = ProviderErrorOptions {
        provider: PROVIDER.to_owned(),
        ..ProviderErrorOptions::default()
    };
    ProviderError::new(message.to_owned(), options)
}

/// The body each of the `n` requests sends, as Python's `_build_body`
/// writes it: the size as pixels, the configuration's controls, the
/// references.
fn body_of(
    config: &OpenRouterImageConfig,
    prompt: &str,
    size: Option<&str>,
    references: &[AiImagePart],
) -> Result<Value, ImageRequestError> {
    let mut body = Map::new();
    body.insert("model".to_owned(), config.model.clone().into());
    body.insert("prompt".to_owned(), prompt.into());
    if let Some(size) = size {
        let (width, height) = parse_size(size)?;
        body.insert("size".to_owned(), format!("{width}x{height}").into());
    }
    let controls = [
        ("quality", &config.quality),
        ("background", &config.background),
        ("output_format", &config.output_format),
    ];
    for (name, value) in controls {
        if let Some(value) = value {
            body.insert(name.to_owned(), value.clone().into());
        }
    }
    if !references.is_empty() {
        let references: Vec<Value> = references
            .iter()
            .enumerate()
            .map(|(at, part)| reference_of(part, at))
            .collect::<Result<_, _>>()?;
        body.insert("input_references".to_owned(), references.into());
    }
    Ok(Value::Object(body))
}

/// A reference image as `input_references` takes it: a remote URL as it
/// is, for OpenRouter to fetch; inline bytes decoded, which proves them,
/// and written again as a canonical data URI (Python's `_reference`).
fn reference_of(part: &AiImagePart, at: usize) -> Result<Value, ImageRequestError> {
    if !part.url.starts_with("data:") {
        return Ok(json!({ "type": "image_url", "image_url": { "url": part.url } }));
    }
    let options = ParseDataUriOptions {
        fallback_mime: part.mime_type.clone(),
    };
    let (mime_type, data) = parse_data_uri(&part.url, options)
        .map_err(|unread| ImageRequestError::new(format!("reference image {at}: {unread}")))?;
    let url = to_data_uri(&data, &mime_type);
    Ok(json!({ "type": "image_url", "image_url": { "url": url } }))
}

/// The one image of an answer (Python's `_result`), with its request's
/// usage.
fn result_of(payload: &Map<String, Value>) -> Result<ImageResult, ProviderError> {
    let images = payload.get("data").and_then(Value::as_array);
    let images = images.map(Vec::as_slice).unwrap_or_default();
    let [image] = images else {
        let count = images.len();
        let said = format!("OpenRouter returned {count} image(s) for a request of 1");
        return Err(own_failure(&said));
    };
    let payload_bytes = image.get("b64_json").and_then(Value::as_str);
    let Some(data) = payload_bytes.filter(|data| !data.is_empty()) else {
        return Err(own_failure(
            "OpenRouter returned an image without inline bytes; the Image API answers in \
             base64, so an empty payload is a failed generation",
        ));
    };
    // The media type it declares, else the one its bytes show.
    let declared = image.get("media_type").and_then(Value::as_str);
    let mime_type = payload_mime_type(declared, data, PayloadMimeTypeOptions::default());
    let uri = format!("data:{mime_type};base64,{data}");
    let mut result =
        ImageResult::new(uri, mime_type).map_err(|refused| own_failure(&refused.to_string()))?;
    result.revised_prompt = image
        .get("revised_prompt")
        .and_then(Value::as_str)
        .map(str::to_owned);
    result.usage = usage_of(payload.get("usage"));
    Ok(result)
}

/// OpenRouter's usage as the counters RFC §25.5 names, and the bill: the
/// prompt's tokens as the text input, the completion's as the image's, and
/// the amount OpenRouter billed for the call, in USD, under its own key
/// (Python's `_usage`).
fn usage_of(usage: Option<&Value>) -> Metadata {
    let mut counters = Metadata::new();
    let Some(Value::Object(usage)) = usage else {
        return counters;
    };
    let whole = |value: &Value| value.as_i64().or_else(|| value.as_f64().map(f64_whole));
    let counted = [
        ("prompt_tokens", "input_tokens"),
        ("completion_tokens", "output_image_tokens"),
    ];
    for (said, counter) in counted {
        if let Some(count) = usage.get(said).and_then(whole) {
            counters.insert(counter.to_owned(), count.into());
        }
    }
    if let Some(cost) = usage.get("cost").and_then(Value::as_f64) {
        counters.insert("cost".to_owned(), cost.into());
    }
    counters
}

/// A count written as a fraction, whole as Python's `int` makes it.
#[expect(
    clippy::cast_possible_truncation,
    reason = "Python's int() of a float truncates; a count past 64 bits saturates"
)]
fn f64_whole(value: f64) -> i64 {
    value.trunc() as i64
}

impl ImageProvider for OpenRouterImage {
    fn name(&self) -> &'static str {
        "OpenRouterImageProvider"
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

    #[expect(
        clippy::manual_async_fn,
        reason = "the trait's future, of the prompt's and the provider's lifetime"
    )]
    fn generate<'a>(
        &'a self,
        prompt: &'a str,
        options: GenerateOptions,
    ) -> impl Future<Output = Result<Vec<ImageResult>, ImageError>> + Send + 'a {
        async move { self.generated(prompt, options).await }
    }
}

#[cfg(test)]
mod replay;
#[cfg(test)]
mod tests;
