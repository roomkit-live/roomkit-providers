//! What a provider knows of its models before any request (RFC §6.7):
//! the entries of a model catalogue.
//!
//! - [`ModelInfo`]: one model a provider offers.
//! - [`ModelPricing`]: its list price, with [`ModelPricing::cost_for`] to
//!   price a response's usage, and [`Date`], the day the price was read.
//! - [`speech_tags`] and [`with_speech_tags`]: the capability tags a listing
//!   reads off a speech model's id ([`TRANSCRIPTION_CAPABILITY`],
//!   [`SPEECH_CAPABILITY`]).

mod date;
mod model_tags;
mod pricing;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub use self::date::Date;
pub use self::model_tags::{
    SPEECH_CAPABILITY, TRANSCRIPTION_CAPABILITY, speech_tags, with_speech_tags,
};
pub use self::pricing::ModelPricing;

/// Metadata describing one model a provider offers: Python's
/// `ModelInfo(*, id, display_name=None, context_window=None,
/// supports_vision=None, deprecated=False, capabilities=[], pricing=None)`.
///
/// Only `id` is certain; the rest is what the source states, `None` where
/// it states nothing. A curated catalogue's `capabilities` may hold a
/// provider's own routing tags (`chat_tools_refused`,
/// `tools_reasoning_none`); empty means unknown, not none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    /// The model's exact identifier, as the provider's API takes it.
    pub id: String,
    /// A name for people.
    #[serde(default)]
    pub display_name: Option<String>,
    /// The model's input context window, in tokens.
    #[serde(default)]
    pub context_window: Option<u64>,
    /// Whether the model takes images.
    #[serde(default)]
    pub supports_vision: Option<bool>,
    /// Whether the provider marks the model deprecated.
    #[serde(default)]
    pub deprecated: bool,
    /// The model's capability tags.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// The vendor's list price, where it publishes one.
    #[serde(default)]
    pub pricing: Option<ModelPricing>,
}

impl ModelInfo {
    /// The entry for model `id`, every other field Python's default.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            display_name: None,
            context_window: None,
            supports_vision: None,
            deprecated: false,
            capabilities: Vec::new(),
            pricing: None,
        }
    }
}

/// `live`, the models a provider's API reports, as its listing returns
/// them: backfilled from the provider's `curated` catalogue, its speech
/// models tagged. Python's `AIProvider._listing`, for a provider of another
/// crate.
#[doc(hidden)]
pub fn listing(live: Vec<ModelInfo>, curated: &[ModelInfo]) -> Vec<ModelInfo> {
    with_speech_tags(merge_curated(live, curated))
}

/// What a live endpoint left out filled from the `curated` catalogue, for
/// each model of `live` it carries, what the endpoint did report kept: a
/// display name, a context window, whether it takes images, its price. A
/// catalogue's capabilities stay out: most carry internal routing flags, not
/// a public tag set. Python's `AIProvider._merge_curated`.
fn merge_curated(live: Vec<ModelInfo>, curated: &[ModelInfo]) -> Vec<ModelInfo> {
    let index: HashMap<&str, &ModelInfo> = curated
        .iter()
        .map(|model| (model.id.as_str(), model))
        .collect();
    live.into_iter()
        .map(|model| match index.get(model.id.as_str()) {
            Some(known) => ModelInfo {
                // Python's `or`: an empty name and a window of 0 are none.
                display_name: model
                    .display_name
                    .filter(|name| !name.is_empty())
                    .or_else(|| known.display_name.clone()),
                context_window: model
                    .context_window
                    .filter(|window| *window != 0)
                    .or(known.context_window),
                supports_vision: model.supports_vision.or(known.supports_vision),
                pricing: model.pricing.or_else(|| known.pricing.clone()),
                ..model
            },
            None => model,
        })
        .collect()
}

#[cfg(test)]
mod tests;
