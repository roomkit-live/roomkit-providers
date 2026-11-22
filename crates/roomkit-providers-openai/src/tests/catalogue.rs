//! `OpenAI`'s curated catalogue as the provider embeds it, and what the
//! provider reads of its model's entry.

use std::collections::BTreeSet;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use roomkit_ai::{AiProvider, Date};
use serde_json::Value;

use super::support::official;
use crate::models::{entry, models};
use crate::{OpenAi, OpenAiConfig};

/// The catalogue as Python wrote it.
const CATALOGUE: &str = include_str!("../../catalog/models.json");

// Source: src/models/tests.rs::the_catalogue_reads_whole
#[test]
fn the_catalogue_reads_whole() {
    // The copy writes back as Python wrote it: nothing of Python's entries
    // is lost or added in the reading.
    let python: Value = serde_json::from_str(CATALOGUE).unwrap();
    assert_eq!(serde_json::to_value(models()).unwrap(), python["models"]);
}

// Source: src/models/tests.rs::curated_catalog_is_nonempty_and_unique
#[test]
fn curated_catalog_is_nonempty_and_unique() {
    let models = OpenAi::available_models();
    assert_ne!(models, Vec::new());
    let ids: BTreeSet<&str> = models.iter().map(|model| model.id.as_str()).collect();
    assert_eq!(ids.len(), models.len(), "duplicate model ids");
}

// Source: src/models/tests.rs::every_model_in_a_priced_catalog_carries_a_price
#[test]
fn every_model_in_a_priced_catalog_carries_a_price() {
    for model in models().iter().filter(|model| !model.deprecated) {
        assert!(model.pricing.is_some(), "{} has no price", model.id);
    }
}

/// Today, in UTC.
fn today() -> Date {
    let now = Timestamp::now().to_zoned(TimeZone::UTC).date();
    let year = u16::try_from(now.year()).unwrap_or_default();
    let month = u8::try_from(now.month()).unwrap_or_default();
    let day = u8::try_from(now.day()).unwrap_or_default();
    Date::new(year, month, day).unwrap()
}

// Source: src/models/tests.rs::catalog_prices_are_well_formed
#[test]
fn catalog_prices_are_well_formed() {
    let today = today();
    for model in models() {
        let Some(pricing) = &model.pricing else {
            continue;
        };
        assert!(pricing.input_per_million > 0.0, "{} input rate", model.id);
        assert!(pricing.output_per_million > 0.0, "{} output rate", model.id);
        assert!(!pricing.currency.is_empty(), "{} currency", model.id);
        assert!(
            pricing.verified <= today,
            "{} verified in the future",
            model.id
        );
        for rate in [
            pricing.cache_read_per_million,
            pricing.cache_write_per_million,
        ] {
            assert!(
                rate.is_none_or(|rate| rate >= 0.0),
                "{} cache rate",
                model.id
            );
        }
    }
}

// Source: src/models/tests.rs::openai_gpt_5_6_luna_has_its_smaller_untiered_context
#[test]
fn openai_gpt_5_6_luna_has_its_smaller_untiered_context() {
    let luna = entry("gpt-5.6-luna").unwrap();
    assert_eq!(luna.context_window, Some(400_000));
    let pricing = luna.pricing.as_ref().unwrap();
    assert_eq!(pricing.long_context_threshold_tokens, None);
}

// Source: src/models/tests.rs::the_provider_reads_its_model_s_entry
#[test]
fn the_provider_reads_its_model_s_entry() {
    let provider = OpenAi::new(official("gpt-6-luna")).unwrap();
    let entry = provider.catalog_entry().unwrap();
    assert_eq!(entry.id, "gpt-6-luna");
    assert_eq!(provider.context_window(), entry.context_window);
    let local = OpenAi::new(OpenAiConfig::new("", "local-model")).unwrap();
    assert_eq!(local.catalog_entry(), None);
    assert_eq!(local.context_window(), None);
}
