//! What an interaction gives, read as Python's `GeminiImageProvider` reads
//! it through google-genai: the image and the text of its last model
//! output, as the SDK derives `output_image` and `output_text` from its
//! steps; the usage split into the counters RFC §25.5 names; and what the
//! attempt keeps of it.

use roomkit::model::Metadata;
use roomkit_ai::image::{ImageAttempt, ImageResult, PayloadMimeTypeOptions, payload_mime_type};
use roomkit_ai::{ProviderError, ProviderErrorOptions};
use serde_json::{Map, Value};

use super::fields;
use super::repr::{repr, value_repr};

/// The name of the provider's failures.
pub(crate) const PROVIDER: &str = "gemini";

/// A failure of the provider's own, in its words: final.
pub(crate) fn own_failure(message: String) -> ProviderError {
    let options = ProviderErrorOptions {
        provider: PROVIDER.to_owned(),
        ..ProviderErrorOptions::default()
    };
    ProviderError::new(message, options)
}

/// What google-genai makes of an answer: it refuses no JSON. An object its
/// models validate is an interaction; one they do not is built member by
/// member (`construct_unvalidated`), its defaults where a member is
/// missing; a value that is no object is kept as it is, which has none of
/// an interaction's members.
enum Built<'a> {
    Validated(&'a Map<String, Value>),
    Constructed(&'a Map<String, Value>),
    Kept,
}

impl<'a> Built<'a> {
    /// What the SDK makes of `answer`. An object is taken as validated when
    /// the members the provider reads are of the types the SDK's models
    /// declare: its required `status` a text, its `steps` a list of objects
    /// that each have a `type`, its `id` and `output_text` texts, its
    /// `usage` and `output_image` objects. The SDK checks every member;
    /// the provider reads only these.
    fn of(answer: &'a Value) -> Self {
        let Value::Object(members) = answer else {
            return Self::Kept;
        };
        let is = |name: &str, typed: fn(&Value) -> bool| {
            members
                .get(name)
                .is_none_or(|value| value.is_null() || typed(value))
        };
        let steps_named = members
            .get("steps")
            .and_then(Value::as_array)
            .is_none_or(|steps| {
                steps.iter().all(|step| {
                    step.as_object()
                        .is_some_and(|step| step.contains_key("type"))
                })
            });
        let validated = members.get("status").is_some_and(Value::is_string)
            && is("id", Value::is_string)
            && is("output_text", Value::is_string)
            && is("steps", Value::is_array)
            && is("usage", Value::is_object)
            && is("output_image", Value::is_object)
            && steps_named;
        if validated {
            Self::Validated(members)
        } else {
            Self::Constructed(members)
        }
    }

    fn members(&self) -> Option<&'a Map<String, Value>> {
        match self {
            Self::Validated(members) | Self::Constructed(members) => Some(members),
            Self::Kept => None,
        }
    }

    fn member(&self, name: &str) -> Option<&'a Value> {
        self.members()?.get(name).filter(|value| !value.is_null())
    }

    /// The interaction's id: an empty one when it states none, the SDK's
    /// default; none when it states `null`.
    fn id(&self) -> Option<String> {
        let members = self.members()?;
        match members.get("id") {
            None => Some(String::new()),
            Some(Value::Null) => None,
            Some(id) => id.as_str().map(str::to_owned),
        }
    }

    /// The steps the attempt keeps: none for an interaction that states
    /// none; an empty list for a value that is no object, which the
    /// provider reads with a default.
    fn kept_steps(&self) -> Value {
        match self.member("steps") {
            Some(Value::Array(steps)) => fields::steps(steps),
            Some(other) => fields::raw(other),
            None if self.members().is_some() => Value::Null,
            None => Value::Array(Vec::new()),
        }
    }

    /// The steps the SDK reads its outputs from: a list, or none.
    fn steps(&self) -> &'a [Value] {
        let steps = self.member("steps").and_then(Value::as_array);
        steps.map(Vec::as_slice).unwrap_or_default()
    }

    /// The SDK's `output_text`: the text the steps end with; else an empty
    /// one for an interaction its models validated, which always derive
    /// it, and what the answer states for one built member by member.
    fn output_text(&self) -> Value {
        let text = output_text(self.steps());
        if !text.is_empty() {
            return text.into();
        }
        match self {
            Self::Validated(_) => text.into(),
            Self::Constructed(_) => self.member("output_text").map_or(Value::Null, fields::raw),
            Self::Kept => Value::Null,
        }
    }

    /// The SDK's `output_image`: the last image of the steps, else the one
    /// the answer states.
    fn output_image(&self) -> Option<&'a Value> {
        output_image(self.steps()).or_else(|| {
            self.member("output_image")
                .filter(|image| image.is_object())
        })
    }

    /// The interaction's status as Python's `repr` writes it.
    fn status(&self) -> String {
        match self.member("status") {
            None => "None".to_owned(),
            Some(Value::String(status)) => repr(status),
            Some(other) => value_repr(other),
        }
    }
}

/// Reads `interaction`, an answer of the Interactions API, into `attempt`,
/// as Python's provider reads what google-genai made of it: its id, its
/// usage, its counters as written and its steps, then its image.
///
/// # Errors
///
/// An interaction that holds no image, or a link in place of one, in the
/// provider's words: final, the image generated and billed.
pub(crate) fn read(interaction: &Value, attempt: &mut ImageAttempt) -> Result<(), ProviderError> {
    let built = Built::of(interaction);
    attempt.provider_request_id = built.id();
    let usage = built.member("usage").filter(|usage| usage.is_object());
    attempt.usage = usage_of(usage);
    attempt.raw_usage = match usage.map(fields::usage) {
        Some(Value::Object(members)) => members,
        _ => Metadata::new(),
    };
    let mut metadata = Metadata::new();
    metadata.insert("steps".to_owned(), built.kept_steps());
    metadata.insert("output_text".to_owned(), built.output_text());
    attempt.metadata = metadata;
    let result = result_of(&built, attempt)?;
    attempt.results = vec![result];
    Ok(())
}

/// The image of `built` as the attempt keeps it (Python's `_result`).
fn result_of(built: &Built<'_>, attempt: &ImageAttempt) -> Result<ImageResult, ProviderError> {
    let image = built.output_image();
    let payload = image
        .and_then(|image| image.get("data"))
        .and_then(Value::as_str);
    let Some(payload) = payload.filter(|payload| !payload.is_empty()) else {
        let uri = image
            .and_then(|image| image.get("uri"))
            .and_then(Value::as_str);
        if uri.is_some_and(|uri| !uri.is_empty()) {
            return Err(own_failure(
                "Gemini delivered the image as a URI; roomkit returns inline bytes only, since \
                 a link expires while an ImageResult is expected to outlive the call"
                    .to_owned(),
            ));
        }
        let status = built.status();
        let said = format!("Gemini returned an interaction with no image; status={status}");
        return Err(own_failure(said));
    };
    // An undeclared type is read off the bytes, never assumed: Nano Banana
    // 2.1 draws JPEG only.
    let declared = image
        .and_then(|image| image.get("mime_type"))
        .and_then(Value::as_str);
    let mime_type = payload_mime_type(declared, payload, PayloadMimeTypeOptions::default());
    let data = format!("data:{mime_type};base64,{payload}");
    let mut result =
        ImageResult::new(data, mime_type).map_err(|refused| own_failure(refused.to_string()))?;
    // Gemini rewrites no prompt: its text is about the picture.
    result.usage.clone_from(&attempt.usage);
    result.attempt_id = Some(attempt.id.clone());
    result
        .provider_request_id
        .clone_from(&attempt.provider_request_id);
    result.raw_usage.clone_from(&attempt.raw_usage);
    result
        .effective_options
        .clone_from(&attempt.effective_options);
    result.metadata.clone_from(&attempt.metadata);
    Ok(result)
}

/// The model outputs since the last user input, latest first: what the SDK
/// reads an interaction's output from.
fn outputs(steps: &[Value]) -> impl Iterator<Item = &Value> {
    steps
        .iter()
        .rev()
        .take_while(|step| kind(step) != Some("user_input"))
}

fn kind(value: &Value) -> Option<&str> {
    value.get("type").and_then(Value::as_str)
}

fn content(step: &Value) -> &[Value] {
    let content = step.get("content").and_then(Value::as_array);
    content.map(Vec::as_slice).unwrap_or_default()
}

/// The last image of the model's outputs: the SDK's `output_image`.
fn output_image(steps: &[Value]) -> Option<&Value> {
    outputs(steps)
        .filter(|step| kind(step) == Some("model_output"))
        .flat_map(|step| content(step).iter().rev())
        .find(|item| kind(item) == Some("image"))
}

/// The text the model's outputs end with: the SDK's `output_text`, its
/// trailing text parts joined, read back from the end until a part that is
/// no text, a thought skipped, any other step ending the text once begun;
/// empty for none.
fn output_text(steps: &[Value]) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut collecting = false;
    'steps: for step in outputs(steps) {
        match kind(step) {
            Some("thought") => continue,
            Some("model_output") => {}
            _ if collecting => break,
            _ => continue,
        }
        for item in content(step).iter().rev() {
            if kind(item) != Some("text") {
                if collecting {
                    break 'steps;
                }
                continue;
            }
            collecting = true;
            parts.push(item.get("text").and_then(Value::as_str).unwrap_or_default());
        }
    }
    parts.iter().rev().copied().collect()
}

/// Python's `gemini_image_usage`: the counters of an interaction's usage,
/// disjoint. A breakdown by modality gives the image share, the text the
/// rest of the total; thought tokens, billed as text output, join it.
pub(crate) fn usage_of(usage: Option<&Value>) -> Metadata {
    let mut counted = Metadata::new();
    let Some(usage) = usage else {
        return counted;
    };
    let count = |name: &str| usage.get(name).and_then(Value::as_i64);
    for direction in ["input", "output"] {
        let total = count(&format!("total_{direction}_tokens"));
        let breakdown = usage.get(format!("{direction}_tokens_by_modality"));
        let breakdown = breakdown.filter(|breakdown| !breakdown.is_null());
        let Some(breakdown) = breakdown else {
            if let Some(total) = total {
                counted.insert(format!("unclassified_{direction}_tokens"), total.into());
            }
            continue;
        };
        let counts = by_modality(breakdown);
        let tokens = |modality: &str| {
            counts
                .iter()
                .find(|(seen, _)| seen == modality)
                .map(|(_, tokens)| *tokens)
        };
        let image = tokens("image").unwrap_or_default();
        counted.insert(format!("{direction}_image_tokens"), image.into());
        if let Some(total) = total {
            counted.insert(
                format!("{direction}_tokens"),
                total.saturating_sub(image).max(0).into(),
            );
        } else if let Some(text) = tokens("text") {
            counted.insert(format!("{direction}_tokens"), text.into());
        }
    }
    if let Some(thoughts) = count("total_thought_tokens") {
        let output = counted
            .get("output_tokens")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        counted.insert(
            "output_tokens".to_owned(),
            output.saturating_add(thoughts).into(),
        );
    }
    if let Some(cached) = count("total_cached_tokens").filter(|cached| *cached != 0) {
        counted.insert("unclassified_cached_tokens".to_owned(), cached.into());
    }
    counted
}

/// The tokens of a breakdown summed by modality, lower-cased as Python's
/// `str(modality).lower()` reads it, a missing one `none`.
fn by_modality(breakdown: &Value) -> Vec<(String, i64)> {
    let mut counts: Vec<(String, i64)> = Vec::new();
    for entry in breakdown.as_array().into_iter().flatten() {
        let Some(tokens) = entry.get("tokens").and_then(Value::as_i64) else {
            continue;
        };
        let modality = match entry.get("modality") {
            Some(Value::String(modality)) => modality.to_lowercase(),
            _ => "none".to_owned(),
        };
        match counts.iter_mut().find(|(seen, _)| *seen == modality) {
            Some((_, counted)) => *counted = counted.saturating_add(tokens),
            None => counts.push((modality, tokens)),
        }
    }
    counts
}
