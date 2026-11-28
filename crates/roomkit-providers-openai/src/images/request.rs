//! What an image request asks of `OpenAI`'s images API, checked before it is
//! sent, as Python's `OpenAIImageProvider.generate_with_options` checks it:
//! its size, its controls merged over the configuration's, what the model
//! takes, then the body, JSON for a generation, multipart for an edit.

use roomkit::model::Metadata;
use roomkit_ai::AiImagePart;
use roomkit_ai::image::{
    ImageCatalogEntry, ImageOptions, ImageRequestError, ParseDataUriOptions, parse_data_uri,
    parse_size,
};
use roomkit_providers_client::multipart::{self, Part};
use serde_json::{Map, Value};

use super::OpenAiImageConfig;

/// The members of a generation's body, in the order `OpenAI`'s Python SDK
/// writes them (`images.generate`).
const GENERATION_ORDER: [&str; 19] = [
    "model",
    "stream",
    "prompt",
    "background",
    "complexity",
    "frame_count",
    "frame_duration_ms",
    "moderation",
    "n",
    "output_compression",
    "output_format",
    "partial_images",
    "quality",
    "response_format",
    "safety_identifier",
    "size",
    "style",
    "svg",
    "user",
];

/// The fields of an edit's body, in the order the SDK writes them
/// (`images.edit`): its files go last, the reference images then the mask.
const EDIT_ORDER: [&str; 19] = [
    "prompt",
    "background",
    "complexity",
    "frame_count",
    "frame_duration_ms",
    "input_fidelity",
    "markup",
    "model",
    "n",
    "output_compression",
    "output_format",
    "partial_images",
    "quality",
    "response_format",
    "safety_identifier",
    "size",
    "stream",
    "svg",
    "user",
];

/// The controls a configuration sets for every request.
const CONFIGURED: [&str; 3] = ["quality", "background", "output_format"];

/// What a request asks, as one call of the provider.
pub(crate) struct Asked<'a> {
    /// The prompt.
    pub(crate) prompt: &'a str,
    /// The size, `WIDTHxHEIGHT` or `auto`.
    pub(crate) size: Option<&'a str>,
    /// How many images.
    pub(crate) n: u32,
    /// The images to edit.
    pub(crate) references: &'a [AiImagePart],
    /// The controls the request gives.
    pub(crate) options: Option<&'a ImageOptions>,
    /// The region to repaint.
    pub(crate) mask: Option<&'a AiImagePart>,
    /// Whether a progress callback is told of each attempt.
    pub(crate) told: bool,
}

/// A request checked: what is sent, and what its attempt says it ran with.
pub(crate) struct Checked {
    /// Whether it is an edit, sent to the edits endpoint.
    pub(crate) edit: bool,
    /// Whether its answer is a stream of previews.
    pub(crate) stream: bool,
    /// The options it runs with, as Python's attempt writes them.
    pub(crate) effective: Metadata,
    /// Its body's content type, and its body.
    pub(crate) body: (String, Vec<u8>),
}

/// A request's size as the images API takes it: `auto`, or two whole
/// numbers joined by `x` (Python's `_validated_size`).
fn validated_size(size: &str) -> Result<String, ImageRequestError> {
    if size == "auto" {
        return Ok(size.to_owned());
    }
    let (width, height) = parse_size(size)?;
    Ok(format!("{width}x{height}"))
}

/// The request's controls over the configuration's, as Python merges them
/// (`ImageOptions.model_validate({**defaults, **options})`), and the names
/// of the configuration's.
fn merged(
    config: &OpenAiImageConfig,
    options: Option<&ImageOptions>,
) -> Result<(ImageOptions, Vec<&'static str>), ImageRequestError> {
    let configured = [&config.quality, &config.background, &config.output_format];
    let mut controls = Map::new();
    let mut defaults = Vec::new();
    for (name, value) in CONFIGURED.into_iter().zip(configured) {
        if let Some(value) = value {
            controls.insert(name.to_owned(), Value::String(value.clone()));
            defaults.push(name);
        }
    }
    let given = serde_json::to_value(options.cloned().unwrap_or_default());
    let given = given.map_err(|error| ImageRequestError::new(error.to_string()))?;
    for (name, value) in given.as_object().into_iter().flatten() {
        if !value.is_null() {
            controls.insert(name.clone(), value.clone());
        }
    }
    let merged: ImageOptions = serde_json::from_value(Value::Object(controls))
        .map_err(|error| ImageRequestError::new(error.to_string()))?;
    merged.out_of_bounds()?;
    Ok((merged, defaults))
}

/// Refuses what the model cannot be asked, before any request: what its
/// catalogue entry takes, or, for a model none describes, any control but
/// the configuration's and any mask; then what `OpenAI`'s endpoint refuses
/// whatever the model.
fn refuse(
    asked: &Asked<'_>,
    size: Option<&str>,
    options: &ImageOptions,
    defaults: &[&str],
    entry: Option<&ImageCatalogEntry>,
) -> Result<(), ImageRequestError> {
    if let Some(image) = entry.and_then(ImageCatalogEntry::image) {
        let references = asked.references.len();
        image.validate_request(options, size, asked.n, references, asked.mask.is_some())?;
    } else {
        let unknown = options
            .supplied()
            .iter()
            .any(|name| !defaults.contains(name));
        if unknown || asked.mask.is_some() {
            return Err(ImageRequestError::new(
                "Advanced controls require a model with known image capabilities",
            ));
        }
    }
    if !(1..=10).contains(&asked.n) {
        return Err(ImageRequestError::new(
            "n must be at least 1 and at most 10",
        ));
    }
    if options.input_fidelity.is_some() && asked.references.is_empty() {
        return Err(ImageRequestError::new(
            "input_fidelity requires reference images",
        ));
    }
    if options.partial_images.is_some() && !asked.told {
        return Err(ImageRequestError::new(
            "Streaming previews require on_progress",
        ));
    }
    Ok(())
}

/// What `asked` sends to the provider of `config`, the model's catalogue
/// entry `entry`, checked as Python checks it.
pub(crate) fn checked(
    config: &OpenAiImageConfig,
    asked: &Asked<'_>,
    entry: Option<&ImageCatalogEntry>,
) -> Result<Checked, ImageRequestError> {
    let size = asked.size.map(validated_size).transpose()?;
    let (options, defaults) = merged(config, asked.options)?;
    refuse(asked, size.as_deref(), &options, &defaults, entry)?;
    // Python's keywords: the model, the count, the controls, the size.
    let mut effective = Metadata::new();
    effective.insert("model".to_owned(), Value::String(config.model.clone()));
    effective.insert("n".to_owned(), Value::from(asked.n));
    let controls = serde_json::to_value(&options).unwrap_or_default();
    for (name, value) in controls.as_object().into_iter().flatten() {
        if !value.is_null() {
            effective.insert(name.clone(), value.clone());
        }
    }
    if let Some(size) = &size {
        effective.insert("size".to_owned(), Value::String(size.clone()));
    }
    let uploads = asked
        .references
        .iter()
        .enumerate()
        .map(|(at, part)| upload(part, at))
        .collect::<Result<Vec<_>, _>>()?;
    let mask = asked.mask.map(|mask| upload(mask, 0)).transpose()?;
    if mask.as_ref().is_some_and(|mask| mask.2 != "image/png") {
        return Err(ImageRequestError::new("A mask must be a PNG image"));
    }
    let edit = !asked.references.is_empty();
    let stream = options.partial_images.is_some();
    let body = if edit {
        edit_body(asked.prompt, &effective, stream, &uploads, mask)
    } else {
        generation_body(asked.prompt, &effective, stream)
    };
    Ok(Checked {
        edit,
        stream,
        effective,
        body,
    })
}

/// A reference image as the edits endpoint uploads it, Python's
/// `_as_upload`: its file name, its bytes and its media type.
fn upload(part: &AiImagePart, at: usize) -> Result<(String, Vec<u8>, String), ImageRequestError> {
    let options = ParseDataUriOptions {
        fallback_mime: part.mime_type.clone(),
    };
    let (mime_type, data) = parse_data_uri(&part.url, options).map_err(|unread| {
        ImageRequestError::new(format!(
            "reference image {at}: {unread}. OpenAI image editing uploads file content, so a \
             reference must carry inline bytes as a data: URI."
        ))
    })?;
    let extension = match mime_type.as_str() {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => "png",
    };
    Ok((format!("reference-{at}.{extension}"), data, mime_type))
}

/// A generation's JSON body: its members in the SDK's order.
fn generation_body(prompt: &str, effective: &Metadata, stream: bool) -> (String, Vec<u8>) {
    let mut body = Map::new();
    for name in GENERATION_ORDER {
        let value = match name {
            "prompt" => Some(Value::String(prompt.to_owned())),
            "stream" => stream.then_some(Value::Bool(true)),
            _ => effective.get(name).cloned(),
        };
        if let Some(value) = value {
            body.insert(name.to_owned(), value);
        }
    }
    let written = serde_json::to_vec(&Value::Object(body)).unwrap_or_default();
    ("application/json".to_owned(), written)
}

/// An edit's multipart body: its fields in the SDK's order, then a
/// moderation, which Python sends apart (`extra_body`), then the reference
/// images and the mask.
fn edit_body(
    prompt: &str,
    effective: &Metadata,
    stream: bool,
    uploads: &[(String, Vec<u8>, String)],
    mask: Option<(String, Vec<u8>, String)>,
) -> (String, Vec<u8>) {
    let mut parts = Vec::new();
    for name in EDIT_ORDER {
        let value = match name {
            "prompt" => Some(Value::String(prompt.to_owned())),
            "stream" => stream.then_some(Value::Bool(true)),
            _ => effective.get(name).cloned(),
        };
        if let Some(value) = value {
            parts.push(Part::field(name, field_text(&value)));
        }
    }
    if let Some(moderation) = effective.get("moderation") {
        parts.push(Part::field("moderation", field_text(moderation)));
    }
    for (filename, data, mime_type) in uploads {
        parts.push(Part::file("image[]", filename, mime_type, data.clone()));
    }
    if let Some((filename, data, mime_type)) = mask {
        parts.push(Part::file("mask", filename, mime_type, data));
    }
    multipart::encoded(&parts)
}

/// A value as a multipart field writes it: a text as it is, a number or a
/// switch as Python's client writes one.
fn field_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other @ (Value::Null
        | Value::Bool(_)
        | Value::Number(_)
        | Value::Array(_)
        | Value::Object(_)) => other.to_string(),
    }
}
