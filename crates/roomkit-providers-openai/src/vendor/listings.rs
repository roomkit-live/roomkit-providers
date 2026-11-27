//! The listings of the vendors whose endpoint lists its models otherwise
//! than `OpenAI`'s `/models` page: OpenRouter's routed models, each with its
//! name, its window and the modalities it reads (Python's
//! `OpenRouterAIProvider.list_models`), and a LiteLLM proxy's deployments,
//! from `/model/info`, folded by public name with the proxy's prices
//! (Python's `LiteLLMAIProvider.list_models`). A page whose `data` is no
//! list lists nothing, as Python's does.

use roomkit_ai::{Date, ModelInfo, ModelPricing, SPEECH_CAPABILITY, TRANSCRIPTION_CAPABILITY};
use serde_json::Value;

use super::Vendor;
use crate::models;

/// The path of LiteLLM's listing, beside `OpenAI`'s `/models`.
pub(crate) const MODEL_INFO: &str = "/model/info";

/// The seconds of a day.
const DAY: f64 = 86_400.0;

impl Vendor {
    /// The models `page` lists, read as the vendor's endpoint writes them:
    /// `None` for a page that does not read; a LiteLLM price dated by `now`,
    /// the seconds since 1970 by the provider's clock.
    pub(crate) fn listed(self, page: &[u8], now: f64) -> Option<Vec<ModelInfo>> {
        if !matches!(self, Self::OpenRouter | Self::LiteLlm) {
            return models::listed(page);
        }
        let page: Value = serde_json::from_slice(crate::failure::past_mark(page)).ok()?;
        if self == Self::OpenRouter {
            return data(&page).iter().map(routed).collect();
        }
        Some(deployments(data(&page), day_of(now)))
    }
}

/// The `data` of a listing page; nothing where it holds no list.
fn data(page: &Value) -> &[Value] {
    page.get("data")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// One of OpenRouter's `/models` entries (Python's `_parse_model`): whether
/// it reads images as its `architecture.input_modalities` say, unknown where
/// they say nothing. `None` for an entry without an id.
fn routed(item: &Value) -> Option<ModelInfo> {
    let mut model = ModelInfo::new(item.get("id")?.as_str()?);
    let modalities = item
        .get("architecture")
        .and_then(|architecture| architecture.get("input_modalities"))
        .and_then(Value::as_array)
        .filter(|modalities| !modalities.is_empty());
    model.display_name = item.get("name").and_then(Value::as_str).map(str::to_owned);
    model.context_window = item.get("context_length").and_then(Value::as_u64);
    model.supports_vision = modalities.map(|modalities| modalities.iter().any(|m| m == "image"));
    Some(model)
}

/// The deployments of a LiteLLM proxy, one model per public name in the
/// order the names first come, each group folded into what all its
/// deployments deliver; an entry without a name is left out.
fn deployments(data: &[Value], today: Option<Date>) -> Vec<ModelInfo> {
    let mut models: Vec<ModelInfo> = Vec::new();
    for item in data {
        let Some(name) = item.get("model_name").and_then(Value::as_str) else {
            continue;
        };
        let info = item.get("model_info").filter(|info| info.is_object());
        let parsed = deployment(name, info.unwrap_or(&Value::Null), today);
        match models.iter_mut().find(|model| model.id == name) {
            Some(previous) => *previous = folded(previous, parsed),
            None => models.push(parsed),
        }
    }
    models
}

/// One `/model/info` entry (Python's `_parse_model`): what the proxy's cost
/// map says of it, unknown where it says nothing, and its `mode` as a speech
/// model's tag.
fn deployment(name: &str, info: &Value, today: Option<Date>) -> ModelInfo {
    let mut model = ModelInfo::new(name);
    let mode = info.get("mode").and_then(Value::as_str);
    let tag = match mode {
        Some("audio_transcription") => Some(TRANSCRIPTION_CAPABILITY),
        Some("audio_speech") => Some(SPEECH_CAPABILITY),
        _ => None,
    };
    model.capabilities = tag.into_iter().map(str::to_owned).collect();
    let window = info
        .get("max_input_tokens")
        .filter(|window| !window.is_f64());
    model.context_window = window.and_then(Value::as_u64);
    model.supports_vision = info.get("supports_vision").and_then(Value::as_bool);
    model.pricing = today.and_then(|today| priced(info, today));
    model
}

/// A deployment's price per million tokens (Python's `_parse_pricing`):
/// none unless both base rates are numbers, nor for a pair of zeros, which
/// LiteLLM writes for a cost it does not know; dated `today`, as the rates
/// were read live.
fn priced(info: &Value, today: Date) -> Option<ModelPricing> {
    let rate = |name: &str| per_million(info.get(name));
    let (input, output) = (
        rate("input_cost_per_token")?,
        rate("output_cost_per_token")?,
    );
    if input == 0.0 && output == 0.0 {
        return None;
    }
    let mut pricing = ModelPricing::new(input, output, today).ok()?;
    pricing.cache_read_per_million = rate("cache_read_input_token_cost");
    pricing.cache_write_per_million = rate("cache_creation_input_token_cost");
    Some(pricing)
}

/// A per-token cost as a rate per million; none for what is no number, a
/// boolean included.
fn per_million(value: Option<&Value>) -> Option<f64> {
    value
        .filter(|value| value.is_number())
        .and_then(Value::as_f64)
        .map(|cost| cost * 1_000_000.0)
}

/// Two deployments of one load-balanced name folded into what the group
/// promises (Python's `_merge_deployments`): the smaller window, unknown if
/// one is; images refused if one refuses them, read if all read them, else
/// unknown; the tags and the price only where both agree.
fn folded(a: &ModelInfo, b: ModelInfo) -> ModelInfo {
    let mut model = ModelInfo::new(a.id.clone());
    model.context_window = a
        .context_window
        .zip(b.context_window)
        .map(|(a, b)| a.min(b));
    model.supports_vision = match (a.supports_vision, b.supports_vision) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    };
    if a.capabilities == b.capabilities {
        model.capabilities = b.capabilities;
    }
    model.pricing = b
        .pricing
        .filter(|pricing| a.pricing.as_ref() == Some(pricing));
    model
}

/// The day, in UTC, `seconds` since 1970 fall on; none past the dates the
/// core reads.
fn day_of(seconds: f64) -> Option<Date> {
    let days = (seconds / DAY).floor();
    if !days.is_finite() || days.abs() > 1.0e9 {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a finite whole number of days within a billion is an i64 as it is"
    )]
    let (year, month, day) = civil(days as i64);
    Date::new(
        u16::try_from(year).ok()?,
        u8::try_from(month).ok()?,
        u8::try_from(day).ok()?,
    )
    .ok()
}

/// The year, month and day of the `days`-th day since 1970-01-01, in the
/// proleptic Gregorian calendar (Howard Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let of_era = shifted.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1_460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_is_read_from_the_seconds_since_1970() {
        let day = |seconds: f64| day_of(seconds).map(|day| day.to_string());
        assert_eq!(day(0.0).as_deref(), Some("1970-01-01"));
        assert_eq!(day(1_767_225_600.0).as_deref(), Some("2026-01-01"));
        assert_eq!(day(1_767_225_599.5).as_deref(), Some("2025-12-31"));
        assert_eq!(day(951_782_400.0).as_deref(), Some("2000-02-29"));
        assert_eq!(day(4_107_456_000.0).as_deref(), Some("2100-02-28"));
        assert_eq!(day(f64::NAN), None);
    }
}
