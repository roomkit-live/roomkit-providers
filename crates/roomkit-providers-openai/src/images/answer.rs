//! What `OpenAI`'s images API answers, read as Python's `OpenAIImageProvider`
//! reads it: the images of a whole answer or of a stream of previews, the
//! usage split into the counters RFC §25.5 names, and what the attempt keeps.

use roomkit::model::Metadata;
use roomkit_ai::image::{ImageAttempt, ImageAttemptStatus, ImageProgressCallback, ImageResult};
use roomkit_ai::{ProviderError, ProviderErrorOptions};
use roomkit_providers_client::Failure;
use serde_json::{Map, Value};

/// The name of the provider's failures, Python's `_provider_name`.
pub(crate) const PROVIDER: &str = "openai";

/// A failure of the provider's own, in its words: final.
pub(crate) fn own_failure(message: String) -> ProviderError {
    let options = ProviderErrorOptions {
        provider: PROVIDER.to_owned(),
        ..ProviderErrorOptions::default()
    };
    ProviderError::new(message, options)
}

/// Python's `openai_image_usage`: the counters of an answer's `usage`,
/// disjoint, none made up for one the answer does not report. The image
/// share of the input is taken off the text's; every output token of the
/// generation endpoint is an image's.
pub(crate) fn usage_of(usage: Option<&Value>) -> Metadata {
    let mut counted = Metadata::new();
    let Some(usage) = usage.filter(|usage| !usage.is_null()) else {
        return counted;
    };
    let count = |value: Option<&Value>| value.and_then(Value::as_i64);
    let total = count(usage.get("input_tokens"));
    let details = usage.get("input_tokens_details");
    let image = count(details.and_then(|details| details.get("image_tokens")));
    let text = count(details.and_then(|details| details.get("text_tokens")));
    if let Some(image) = image {
        counted.insert("input_image_tokens".to_owned(), image.into());
    }
    match (text, total, image) {
        (Some(text), _, _) => {
            counted.insert("input_tokens".to_owned(), text.into());
        }
        (None, Some(total), Some(image)) => {
            counted.insert("input_tokens".to_owned(), (total - image).max(0).into());
        }
        (None, Some(total), None) => {
            counted.insert("unclassified_input_tokens".to_owned(), total.into());
        }
        (None, None, _) => {}
    }
    if let Some(output) = count(usage.get("output_tokens")) {
        counted.insert("output_tokens".to_owned(), 0.into());
        counted.insert("output_image_tokens".to_owned(), output.into());
    }
    counted
}

/// Python's `plain_metadata` of an answer's `usage`: what it says, its
/// empty members left out.
pub(crate) fn raw_usage_of(usage: Option<&Value>) -> Metadata {
    match usage.map(plain) {
        Some(Value::Object(members)) => members,
        _ => Metadata::new(),
    }
}

/// `value` without its null members, at every depth.
fn plain(value: &Value) -> Value {
    match value {
        Value::Object(members) => Value::Object(
            members
                .iter()
                .filter(|(_, member)| !member.is_null())
                .map(|(name, member)| (name.clone(), plain(member)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(plain).collect()),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => {
            other.clone()
        }
    }
}

/// The media type of an encoding the images API names.
fn media_type_of(format: Option<&str>) -> &'static str {
    match format {
        Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "image/png",
    }
}

/// The text of `name` in `value`, for one that is text and not empty.
fn text_of<'v>(value: &'v Value, name: &str) -> Option<&'v str> {
    value.get(name)?.as_str().filter(|text| !text.is_empty())
}

/// The image `payload`, of the type `mime_type`, as the attempt keeps it:
/// the call's usage on its first image alone, the others reporting none,
/// as Python's provider splits nothing of a usage across images.
fn image_of(
    payload: &str,
    mime_type: &str,
    attempt: &ImageAttempt,
) -> Result<ImageResult, ProviderError> {
    let data = format!("data:{mime_type};base64,{payload}");
    let mut result =
        ImageResult::new(data, mime_type).map_err(|refused| own_failure(refused.to_string()))?;
    let first = attempt.results.is_empty();
    if first {
        result.usage.clone_from(&attempt.usage);
        result.raw_usage.clone_from(&attempt.raw_usage);
    }
    result.attempt_id = Some(attempt.id.clone());
    result
        .provider_request_id
        .clone_from(&attempt.provider_request_id);
    result
        .effective_options
        .clone_from(&attempt.effective_options);
    Ok(result)
}

/// The media type of an answer's images: the encoding it names, else the
/// one its request asked, else PNG (Python's `_results`).
fn answered_type(answer: &Value, attempt: &ImageAttempt) -> String {
    let said = text_of(answer, "output_format");
    if said.is_some() {
        return media_type_of(said).to_owned();
    }
    let asked = attempt
        .effective_options
        .get("output_format")
        .and_then(Value::as_str);
    format!("image/{}", asked.unwrap_or("png"))
}

/// Reads `answer` into `attempt`, as Python's provider does once a call
/// came back: its usage, its counters as written, what it says it ran
/// with, then its images (`_results`), each image placed in the attempt as
/// it is read. `expected` images were asked.
///
/// # Errors
///
/// An image the answer sends no bytes of, or a count other than the one
/// asked, in the provider's own words: the images read stay in the attempt.
pub(crate) fn read(
    answer: &Value,
    expected: u32,
    attempt: &mut ImageAttempt,
) -> Result<(), ProviderError> {
    let usage = answer.get("usage");
    attempt.usage = usage_of(usage);
    attempt.raw_usage = raw_usage_of(usage);
    for name in ["size", "quality", "background", "output_format"] {
        if let Some(value) = answer.get(name).filter(|value| !value.is_null()) {
            attempt
                .effective_options
                .insert(name.to_owned(), value.clone());
        }
    }
    let mime_type = answered_type(answer, attempt);
    let images = answer
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut unsent = Vec::new();
    for (at, image) in images.iter().enumerate() {
        let Some(payload) = text_of(image, "b64_json") else {
            unsent.push(format!(
                "{PROVIDER} returned image {at} without inline bytes"
            ));
            continue;
        };
        let mut result = image_of(payload, &mime_type, attempt)?;
        result.revised_prompt = image
            .get("revised_prompt")
            .and_then(Value::as_str)
            .map(str::to_owned);
        attempt.results.push(result);
    }
    if !unsent.is_empty() {
        return Err(own_failure(unsent.join("; ")));
    }
    if images.len() != usize::try_from(expected).unwrap_or(usize::MAX) {
        let count = images.len();
        let said = format!("{PROVIDER} returned {count} images for a request of {expected}");
        return Err(own_failure(said));
    }
    Ok(())
}

/// What a stream of previews gave so far: its final images and what its
/// last one said of itself (Python's `_ImageStream`).
#[derive(Default)]
pub(crate) struct Previews {
    images: Vec<Value>,
    said: Map<String, Value>,
}

/// What an event of a stream asks of the provider.
pub(crate) enum Event {
    /// A preview, to tell the callback of.
    Preview(Box<ImageAttempt>),
    /// The stream's end, which Python's SDK reads in `[DONE]`.
    Done,
    /// Nothing more to do.
    Read,
}

/// The words of Python's SDK for an error a stream sends with no message.
const STREAM_ERROR: &str = "An error occurred during streaming";

impl Previews {
    /// Reads `data`, the data of one event, into `attempt`, as Python's SDK
    /// and provider read it: the stream's end, a preview to tell, a final
    /// image kept, or the error the stream ends with.
    ///
    /// # Errors
    ///
    /// An event that is no JSON, or an error the stream sends, in the
    /// server's words as the client words them: the images kept stay in
    /// the attempt.
    pub(crate) fn read(
        &mut self,
        data: &str,
        attempt: &mut ImageAttempt,
        told: bool,
    ) -> Result<Event, Failure> {
        if data.starts_with("[DONE]") {
            return Ok(Event::Done);
        }
        let event: Value =
            serde_json::from_str(data).map_err(|error| Failure::malformed(&error))?;
        if let Some(error) = event.get("error").filter(|error| is_truthy(error)) {
            return Err(Failure::stream_error(error, STREAM_ERROR));
        }
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let asked = attempt
            .effective_options
            .get("output_format")
            .and_then(Value::as_str);
        let format = text_of(&event, "output_format").or(asked).unwrap_or("png");
        let mime_type = format!("image/{format}");
        match kind.rsplit('.').next().unwrap_or_default() {
            "partial_image" => {
                let Some(payload) = text_of(&event, "b64_json").filter(|_| told) else {
                    return Ok(Event::Read);
                };
                let data = format!("data:{mime_type};base64,{payload}");
                let mut preview = ImageResult::new(data, mime_type)
                    .map_err(|refused| Failure::from(own_failure(refused.to_string())))?;
                preview.attempt_id = Some(attempt.id.clone());
                let mut told = attempt.clone();
                told.status = ImageAttemptStatus::Preview;
                told.results = vec![preview];
                Ok(Event::Preview(Box::new(told)))
            }
            "completed" => {
                self.completed(&event, &mime_type, attempt)
                    .map_err(Failure::from)?;
                Ok(Event::Read)
            }
            "error" => Err(Failure::stream_error(&event, "Image stream failed")),
            _ => Ok(Event::Read),
        }
    }

    /// Keeps the final image of `event` and what it says of itself: its
    /// bytes, when it sends some, in the attempt.
    fn completed(
        &mut self,
        event: &Value,
        mime_type: &str,
        attempt: &mut ImageAttempt,
    ) -> Result<(), ProviderError> {
        let payload = event.get("b64_json").cloned().unwrap_or(Value::Null);
        self.images.push(serde_json::json!({ "b64_json": payload }));
        self.said = ["usage", "size", "quality", "background", "output_format"]
            .into_iter()
            .filter_map(|name| Some((name.to_owned(), event.get(name)?.clone())))
            .collect();
        let usage = self.said.get("usage");
        attempt.raw_usage = raw_usage_of(usage);
        attempt.usage = usage_of(usage);
        // An image with no bytes is none: the answer then says so.
        if let Some(payload) = text_of(event, "b64_json") {
            let result = image_of(payload, mime_type, attempt)?;
            attempt.results.push(result);
        }
        Ok(())
    }

    /// The stream as the whole answer Python's provider then reads: its
    /// images and what its last one said.
    pub(crate) fn answer(self) -> Value {
        let mut answer = self.said;
        answer.insert("data".to_owned(), Value::Array(self.images));
        Value::Object(answer)
    }
}

/// Whether `value` is truthy as Python reads it (`data.get("error")`).
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(members) => !members.is_empty(),
    }
}

/// Tells `callback` of a preview, as Python's stream does, awaiting the
/// callback itself.
///
/// # Errors
///
/// The callback's failure, in its words.
pub(crate) async fn preview(
    callback: Option<&ImageProgressCallback>,
    attempt: ImageAttempt,
) -> Result<(), ProviderError> {
    let Some(callback) = callback else {
        return Ok(());
    };
    callback(attempt)
        .await
        .map_err(|failed| own_failure(failed.to_string()))
}
