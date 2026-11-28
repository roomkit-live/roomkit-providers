//! What an interaction gives, read as Python's `GeminiImageProvider` reads
//! it through google-genai: the image and the text of its last model
//! output, as the SDK derives `output_image` and `output_text` from its
//! steps; the usage split into the counters RFC §25.5 names; and what the
//! attempt keeps of it.

use std::fmt::Write as _;

use roomkit::model::Metadata;
use roomkit_ai::image::{ImageAttempt, ImageResult, PayloadMimeTypeOptions, payload_mime_type};
use roomkit_ai::{ProviderError, ProviderErrorOptions};
use serde_json::Value;

use super::fields;

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

/// Reads `interaction` into `attempt`, as Python's provider does once an
/// interaction came back: its id, its usage, its counters as written and
/// its steps, then its image.
///
/// # Errors
///
/// An interaction that holds no image, or a link in place of one, in the
/// provider's words: final, the image generated and billed.
pub(crate) fn read(interaction: &Value, attempt: &mut ImageAttempt) -> Result<(), ProviderError> {
    let id = interaction.get("id").and_then(Value::as_str);
    attempt.provider_request_id = id.map(str::to_owned);
    let usage = interaction.get("usage").filter(|usage| !usage.is_null());
    attempt.usage = usage_of(usage);
    attempt.raw_usage = match usage.map(fields::usage) {
        Some(Value::Object(members)) => members,
        _ => Metadata::new(),
    };
    let steps = interaction.get("steps").and_then(Value::as_array);
    let steps = steps.map(Vec::as_slice).unwrap_or_default();
    let mut metadata = Metadata::new();
    metadata.insert("steps".to_owned(), fields::steps(steps));
    metadata.insert("output_text".to_owned(), output_text(steps).into());
    attempt.metadata = metadata;
    let result = result_of(interaction, steps, attempt)?;
    attempt.results = vec![result];
    Ok(())
}

/// The image of `interaction` as the attempt keeps it (Python's `_result`).
fn result_of(
    interaction: &Value,
    steps: &[Value],
    attempt: &ImageAttempt,
) -> Result<ImageResult, ProviderError> {
    let image = output_image(steps);
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
        let status = match interaction.get("status").and_then(Value::as_str) {
            Some(status) => repr(status),
            None => "None".to_owned(),
        };
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
            counted.insert(format!("{direction}_tokens"), (total - image).max(0).into());
        } else if let Some(text) = tokens("text") {
            counted.insert(format!("{direction}_tokens"), text.into());
        }
    }
    if let Some(thoughts) = count("total_thought_tokens") {
        let output = counted
            .get("output_tokens")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        counted.insert("output_tokens".to_owned(), (output + thoughts).into());
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
            Some((_, counted)) => *counted += tokens,
            None => counts.push((modality, tokens)),
        }
    }
    counts
}

/// `text` as Python's `repr` writes a `str`: in single quotes, or double
/// quotes when it holds a single one and no double one, its backslashes,
/// its quote and its control characters escaped.
pub(crate) fn repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut written = String::from(quote);
    for character in text.chars() {
        match character {
            '\\' => written.push_str("\\\\"),
            '\n' => written.push_str("\\n"),
            '\r' => written.push_str("\\r"),
            '\t' => written.push_str("\\t"),
            _ if character == quote => {
                written.push('\\');
                written.push(character);
            }
            _ if character.is_control() => {
                // Writing to a string cannot fail.
                write!(written, "\\x{:02x}", u32::from(character)).ok();
            }
            _ => written.push(character),
        }
    }
    written.push(quote);
    written
}
