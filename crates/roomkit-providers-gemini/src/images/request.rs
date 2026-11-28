//! What an image request asks of Gemini's Interactions API, checked before
//! any request as Python's `GeminiImageProvider` checks it: a size as a
//! ratio and a tier, the configuration's defaults under the request's
//! controls, what the model takes; then the interaction's body, the same
//! for each of the `n` interactions, and for each hop that continues one.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use roomkit::model::Metadata;
use roomkit_ai::AiImagePart;
use roomkit_ai::image::{
    ImageCapabilities, ImageCatalogEntry, ImageOptions, ImageRequestError, ParseDataUriOptions,
    parse_data_uri,
};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::GeminiImageConfig;
use super::answer::repr;
use super::geometry::{ASPECT_RATIOS, TIERS, size_geometry};

/// What a request asks.
pub(crate) struct Asked<'a> {
    pub(crate) prompt: &'a str,
    pub(crate) size: Option<&'a str>,
    pub(crate) n: u32,
    pub(crate) references: &'a [AiImagePart],
    pub(crate) options: Option<&'a ImageOptions>,
    pub(crate) masked: bool,
}

/// A request checked: what its attempts say they ran with, and the input
/// its body carries.
pub(crate) struct Checked {
    /// The request but its input, in the order Python builds it: what each
    /// attempt ran with.
    pub(crate) effective: Metadata,
    /// The prompt and the references, as a user input.
    input: Value,
}

/// The hop a body continues an interaction with: the token it continues
/// from, and the id of the hop that gave it.
pub(crate) struct Continued<'a> {
    pub(crate) token: &'a str,
    pub(crate) owner: Option<&'a str>,
}

impl Checked {
    /// The body of an interaction's first hop, or of a hop that `continued`
    /// it, as Python's SDK writes it (`_continuation_body`): a hop continues
    /// the interaction its token's owner stored, its input left out, unless
    /// the request stores none, which sends its input again.
    pub(crate) fn body(&self, continued: Option<&Continued<'_>>) -> Vec<u8> {
        let mut members = self.effective.clone();
        members.insert("input".to_owned(), self.input.clone());
        if let Some(continued) = continued {
            members.insert("continuation_token".to_owned(), continued.token.into());
            let unstored = members.get("store") == Some(&Value::Bool(false));
            let owner = continued
                .owner
                .filter(|owner| !owner.is_empty() && !unstored);
            if let Some(owner) = owner {
                members.insert("previous_interaction_id".to_owned(), owner.into());
                members.remove("input");
            }
        }
        // A JSON value always writes.
        serde_json::to_vec(&wire(members)).unwrap_or_default()
    }
}

/// What `asked` sends with `config`, its model's catalogue entry `entry`.
///
/// # Errors
///
/// [`ImageRequestError`], Python's words, for what the model cannot be
/// asked.
pub(crate) fn checked(
    config: &GeminiImageConfig,
    asked: &Asked<'_>,
    entry: Option<&ImageCatalogEntry>,
) -> Result<Checked, ImageRequestError> {
    if asked.n < 1 {
        return Err(ImageRequestError::new("n must be at least 1"));
    }
    if asked.masked {
        return Err(ImageRequestError::new(
            "Gemini does not support explicit masks",
        ));
    }
    let options = asked.options.cloned().unwrap_or_default();
    let image = entry.and_then(ImageCatalogEntry::image);
    let resolved = call_options(config, asked, &options, image)?;
    let mut content = vec![json!({ "type": "text", "text": asked.prompt })];
    for (at, part) in asked.references.iter().enumerate() {
        content.push(image_content(part, at)?);
    }
    Ok(Checked {
        effective: request_of(&config.model, &resolved),
        input: json!([{ "type": "user_input", "content": content }]),
    })
}

/// The options a call runs with: the configuration's defaults under the
/// request's, checked against the model's capabilities before any billable
/// request (Python's `_call_options`).
fn call_options(
    config: &GeminiImageConfig,
    asked: &Asked<'_>,
    options: &ImageOptions,
    image: Option<&ImageCapabilities>,
) -> Result<ImageOptions, ImageRequestError> {
    let native = options
        .aspect_ratio
        .as_deref()
        .is_some_and(|ratio| !ratio.is_empty())
        || options.image_size.is_some();
    if asked.size.is_some_and(|size| !size.is_empty()) && native {
        return Err(ImageRequestError::new(
            "Use size or aspect_ratio/image_size, not both",
        ));
    }
    let defaults = inherited(config, asked.size, image)?;
    let mut merged = defaults.clone();
    let given = serde_json::to_value(options).unwrap_or_default();
    let given = given.as_object().into_iter().flatten();
    merged.extend(
        given
            .filter(|(_, value)| !value.is_null())
            .map(|(name, value)| (name.clone(), value.clone())),
    );
    let resolved: ImageOptions = serde_json::from_value(Value::Object(merged))
        .map_err(|unread| ImageRequestError::new(unread.to_string()))?;
    resolved.out_of_bounds()?;
    if let Some(image) = image {
        image.validate_request(&resolved, None, asked.n, asked.references.len(), false)?;
        return Ok(resolved);
    }
    let mut supplied = resolved.supplied().into_iter();
    if supplied.any(|name| !defaults.contains_key(name) && name != "store") {
        return Err(ImageRequestError::new(
            "Advanced controls require a model with known image capabilities",
        ));
    }
    Ok(resolved)
}

/// The options a call inherits from the configuration and from a portable
/// size, each refused under the name the caller set when the model lacks it
/// (Python's `_inherited_options`). The size wins over the configured
/// tier: a caller naming pixels is more specific than a deployment.
fn inherited(
    config: &GeminiImageConfig,
    size: Option<&str>,
    image: Option<&ImageCapabilities>,
) -> Result<Map<String, Value>, ImageRequestError> {
    let mut defaults = Map::new();
    if let Some(tier) = config.image_size.as_deref().filter(|tier| !tier.is_empty()) {
        defaults.insert("image_size".to_owned(), tier.into());
    }
    if let Some(mime) = config
        .output_mime_type
        .as_deref()
        .filter(|mime| !mime.is_empty())
    {
        let format = mime.strip_prefix("image/").unwrap_or(mime);
        defaults.insert("output_format".to_owned(), format.into());
        let lacking = image.filter(|image| !image.formats.iter().any(|offered| offered == format));
        if let Some(image) = lacking {
            let offered: Vec<String> = image
                .formats
                .iter()
                .map(|format| format!("image/{format}"))
                .collect();
            let offered = if offered.is_empty() {
                "none".to_owned()
            } else {
                offered.join(", ")
            };
            let (mime, model) = (repr(mime), &config.model);
            return Err(ImageRequestError::new(format!(
                "output_mime_type {mime} is not offered by {model} (its types: {offered})"
            )));
        }
    }
    if let Some(size) = size.filter(|size| !size.is_empty()) {
        let (ratio, tier) = match image {
            Some(image) => size_geometry(
                size,
                &image.aspect_ratios,
                &image.image_sizes,
                &config.model,
            )?,
            None => size_geometry(size, &ASPECT_RATIOS, &TIERS, "Gemini")?,
        };
        defaults.insert("aspect_ratio".to_owned(), ratio.into());
        defaults.insert("image_size".to_owned(), tier.into());
    }
    Ok(defaults)
}

/// The interaction a call asks but its input, in the order Python builds
/// it (`_build_request`). No delivery mode is named: every image model
/// refuses one, and the inline payload is its default.
fn request_of(model: &str, options: &ImageOptions) -> Metadata {
    let mut response_format = Map::new();
    response_format.insert("type".to_owned(), "image".into());
    if let Some(ratio) = &options.aspect_ratio {
        response_format.insert("aspect_ratio".to_owned(), ratio.clone().into());
    }
    if let Some(tier) = &options.image_size {
        response_format.insert("image_size".to_owned(), written(tier).into());
    }
    if let Some(format) = &options.output_format {
        let mime_type = format!("image/{}", written(format));
        response_format.insert("mime_type".to_owned(), mime_type.into());
    }
    let mut request = Metadata::new();
    request.insert("model".to_owned(), model.into());
    request.insert("response_format".to_owned(), Value::Object(response_format));
    if let Some(store) = options.store {
        request.insert("store".to_owned(), store.into());
    }
    if let Some(previous) = options
        .previous_interaction_id
        .as_ref()
        .filter(|id| !id.is_empty())
    {
        request.insert(
            "previous_interaction_id".to_owned(),
            previous.clone().into(),
        );
    }
    if let Some(searches) = options
        .search_types
        .as_ref()
        .filter(|searches| !searches.is_empty())
    {
        let searches: Vec<String> = searches.iter().map(written).collect();
        let tools = json!([{ "type": "google_search", "search_types": searches }]);
        request.insert("tools".to_owned(), tools);
    }
    if let Some(level) = &options.thinking_level {
        let config = json!({ "thinking_level": written(level) });
        request.insert("generation_config".to_owned(), config);
    }
    request
}

/// A `Literal`'s value, as Python and the wire write it.
fn written<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(text)) => text,
        _ => String::new(),
    }
}

/// A reference image as an interaction's image content: a remote URL as it
/// is, for Google to fetch; inline bytes decoded, which proves them, and
/// written again as canonical base64 (Python's `_image_content`).
fn image_content(part: &AiImagePart, at: usize) -> Result<Value, ImageRequestError> {
    if !part.url.starts_with("data:") {
        let mime_type = part.mime_type.as_deref().filter(|mime| !mime.is_empty());
        let mime_type = mime_type.unwrap_or("image/png");
        return Ok(json!({ "type": "image", "uri": part.url, "mime_type": mime_type }));
    }
    let options = ParseDataUriOptions {
        fallback_mime: part.mime_type.clone(),
    };
    let (mime_type, data) = parse_data_uri(&part.url, options)
        .map_err(|unread| ImageRequestError::new(format!("reference image {at}: {unread}")))?;
    let data = STANDARD.encode(data);
    Ok(json!({ "type": "image", "data": data, "mime_type": mime_type }))
}

/// A body as Python's SDK writes it: `model` first, then every member in
/// the order of its name, at every depth.
fn wire(members: Metadata) -> Value {
    let mut body = Map::new();
    if let Some(model) = members.get("model") {
        body.insert("model".to_owned(), model.clone());
    }
    let rest = members.into_iter().filter(|(name, _)| name != "model");
    body.extend(sorted_members(rest.collect()));
    Value::Object(body)
}

/// `members` in the order of their names, at every depth.
fn sorted_members(members: Map<String, Value>) -> Map<String, Value> {
    let mut members: Vec<(String, Value)> = members.into_iter().collect();
    members.sort_by(|(a, _), (b, _)| a.cmp(b));
    members
        .into_iter()
        .map(|(name, value)| (name, sorted(value)))
        .collect()
}

/// `value` with the members of every object in the order of their names.
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(members) => Value::Object(sorted_members(members)),
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => other,
    }
}
