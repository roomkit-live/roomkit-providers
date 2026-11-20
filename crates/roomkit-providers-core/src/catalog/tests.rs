//! Translated from Python's `tests/test_providers/test_ai_models.py`: what
//! a listing takes from a provider's curated catalogue.

use super::*;

fn day() -> Date {
    Date::new(2026, 8, 5).unwrap()
}

// Python: tests/test_providers/test_ai_models.py::test_merge_curated_backfills_pricing
#[test]
fn merge_curated_backfills_pricing() {
    let pricing = ModelPricing::new(1.0, 2.0, day()).unwrap();
    let curated = [ModelInfo {
        pricing: Some(pricing.clone()),
        ..ModelInfo::new("a")
    }];
    let merged = merge_curated(vec![ModelInfo::new("a"), ModelInfo::new("b")], &curated);
    assert_eq!(
        merged
            .iter()
            .map(|m| m.pricing.as_ref())
            .collect::<Vec<_>>(),
        [Some(&pricing), None]
    );
}

// Python: tests/test_providers/test_ai_models.py::test_merge_curated_backfills_missing_metadata
#[test]
fn merge_curated_backfills_missing_metadata() {
    let known = ModelInfo {
        display_name: Some("A".to_owned()),
        context_window: Some(100),
        supports_vision: Some(true),
        ..ModelInfo::new("a")
    };
    let merged = merge_curated(
        vec![ModelInfo::new("a"), ModelInfo::new("b")],
        std::slice::from_ref(&known),
    );
    assert_eq!(merged, [known, ModelInfo::new("b")]);
}

// Python: tests/test_providers/test_ai_models.py::test_merge_curated_prefers_live_values
#[test]
fn merge_curated_prefers_live_values() {
    let curated = [ModelInfo {
        display_name: Some("Curated".to_owned()),
        context_window: Some(100),
        supports_vision: Some(true),
        ..ModelInfo::new("a")
    }];
    let live = ModelInfo {
        display_name: Some("Live".to_owned()),
        context_window: Some(200),
        supports_vision: Some(false),
        ..ModelInfo::new("a")
    };
    assert_eq!(merge_curated(vec![live.clone()], &curated), [live]);
}

/// Python's `or`: an empty name and a window of 0 read as what the endpoint
/// left out.
#[test]
fn merge_curated_reads_an_empty_live_value_as_none() {
    let known = ModelInfo {
        display_name: Some("A".to_owned()),
        context_window: Some(100),
        ..ModelInfo::new("a")
    };
    let live = ModelInfo {
        display_name: Some(String::new()),
        context_window: Some(0),
        ..ModelInfo::new("a")
    };
    assert_eq!(
        merge_curated(vec![live], std::slice::from_ref(&known)),
        [known]
    );
}

// Python: tests/test_providers/test_ai_models.py::test_a_listing_ends_backfilled_and_speech_tagged
#[test]
fn a_listing_ends_backfilled_and_speech_tagged() {
    let curated = [ModelInfo {
        display_name: Some("A".to_owned()),
        capabilities: vec!["chat_tools_refused".to_owned()],
        ..ModelInfo::new("a")
    }];
    let listed = listing(vec![ModelInfo::new("a"), ModelInfo::new("tts-1")], &curated);
    let read: Vec<(Option<&str>, &[String])> = listed
        .iter()
        .map(|model| (model.display_name.as_deref(), &model.capabilities[..]))
        .collect();
    assert_eq!(
        read,
        [(Some("A"), &[][..]), (None, &["speech".to_owned()][..])]
    );
}

// Python: tests/test_providers/test_ai_models.py::test_merge_curated_keeps_a_catalogs_flags_out_of_a_listing
#[test]
fn merge_curated_keeps_a_catalogs_flags_out_of_a_listing() {
    let curated = [ModelInfo {
        capabilities: vec!["chat_tools_refused".to_owned()],
        ..ModelInfo::new("a")
    }];
    let reported = ModelInfo {
        capabilities: vec!["tools".to_owned()],
        ..ModelInfo::new("b")
    };
    let merged = merge_curated(vec![ModelInfo::new("a"), reported.clone()], &curated);
    assert_eq!(merged, [ModelInfo::new("a"), reported]);
}
