//! Gemini's model catalogue: Python's (`catalog/models.json`, written by
//! `scripts/generate_gemini_catalogue.py`), what the provider reads of a
//! model's entry, and the Developer API's listing as Python reads it.

use std::sync::LazyLock;

use roomkit_ai::ModelInfo;
use roomkit_providers_client::catalogue;
#[cfg(test)]
use roomkit_providers_client::catalogue::tagged;
use serde_json::Value;

const CATALOGUE: &str = include_str!("../catalog/models.json");

/// A model that takes a thinking level (Gemini 3).
pub(crate) const TAKES_LEVELS: &str = "thinking_level";
/// A model that takes `minimal` among its levels.
pub(crate) const TAKES_MINIMAL: &str = "thinking_level_minimal";
/// A model that cannot stop reasoning, which answers 400 to a budget of 0.
pub(crate) const CANNOT_STOP: &str = "thinking_required";

static MODELS: LazyLock<Vec<ModelInfo>> = LazyLock::new(|| read().unwrap_or_default());

fn read() -> serde_json::Result<Vec<ModelInfo>> {
    catalogue::read(CATALOGUE)
}

/// The curated catalogue.
pub(crate) fn models() -> &'static [ModelInfo] {
    &MODELS
}

/// The entry of the catalogue for `model`.
pub(crate) fn entry(model: &str) -> Option<&'static ModelInfo> {
    catalogue::entry(models(), model)
}

/// The capability tags of `entry`, none for a model the catalogue does not
/// carry.
pub(crate) fn capabilities(entry: Option<&ModelInfo>) -> &[String] {
    entry.map_or(&[], |entry| entry.capabilities.as_slice())
}

/// The models a page of the Developer API's listing names (Python's
/// `GeminiAIProvider.list_models`): each by the last segment of its name,
/// kept where the actions it declares hold `generateContent`; where it
/// declares none, kept when it is of the Gemini family and no embedding
/// model. `None` for a page whose `models` is no list.
pub(crate) fn listed(page: &Value) -> Option<Vec<ModelInfo>> {
    let Some(models) = page.get("models") else {
        return Some(Vec::new());
    };
    let curated = |name: &str| entry(name).is_some();
    let mut live = Vec::new();
    for model in models.as_array()? {
        let full = model
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let name = full.rsplit('/').next().unwrap_or_default();
        let actions = model
            .get("supportedGenerationMethods")
            .and_then(Value::as_array);
        let actions = actions.filter(|actions| !actions.is_empty());
        if name.is_empty() {
            continue;
        }
        if let Some(actions) = actions {
            if !actions.iter().any(|action| action == "generateContent") {
                continue;
            }
        } else {
            let generative = curated(name) || name.starts_with("gemini-");
            if !generative || name.contains("embedding") {
                continue;
            }
        }
        let mut read = ModelInfo::new(name);
        let shown = model.get("displayName").and_then(Value::as_str);
        read.display_name = shown.map(str::to_owned);
        read.context_window = model.get("inputTokenLimit").and_then(Value::as_u64);
        live.push(read);
    }
    Some(live)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_reads_whole_and_names_what_the_provider_reads() {
        let whole = read().unwrap();
        assert_eq!(whole.len(), models().len());
        assert!(whole.len() >= 15);
        assert!(tagged(entry("gemini-3.8-flash"), TAKES_LEVELS));
        assert!(tagged(entry("gemini-3.6-flash"), TAKES_MINIMAL));
        assert!(tagged(entry("gemini-3.1-pro-preview"), CANNOT_STOP));
        assert!(!tagged(entry("gemini-2.5-flash"), TAKES_LEVELS));
        assert_eq!(capabilities(entry("no-such-model")).len(), 0);
    }
}
