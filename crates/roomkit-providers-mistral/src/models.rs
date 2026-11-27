//! Mistral's model catalogue: Python's (`catalog/models.json`, written by
//! `scripts/generate_mistral_catalogue.py`), and Mistral's listing as Python
//! reads it.

use std::sync::LazyLock;

use roomkit_ai::ModelInfo;
use roomkit_providers_client::catalogue;
use serde_json::Value;

const CATALOGUE: &str = include_str!("../catalog/models.json");

static MODELS: LazyLock<Vec<ModelInfo>> = LazyLock::new(|| read().unwrap_or_default());

fn read() -> serde_json::Result<Vec<ModelInfo>> {
    catalogue::read(CATALOGUE)
}

/// The curated catalogue.
pub(crate) fn models() -> &'static [ModelInfo] {
    &MODELS
}

/// The models a page of Mistral's listing names (Python's
/// `MistralAIProvider.list_models`): each entry of its `data` by its id,
/// an entry without one left out. `None` for a page whose `data` is no
/// list.
pub(crate) fn listed(page: &Value) -> Option<Vec<ModelInfo>> {
    let Some(data) = page.get("data").filter(|data| !data.is_null()) else {
        return Some(Vec::new());
    };
    let ids = data.as_array()?.iter().filter_map(|model| {
        let id = model.get("id").and_then(Value::as_str);
        id.filter(|id| !id.is_empty())
    });
    Some(ids.map(ModelInfo::new).collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_catalogue_reads_whole() {
        let whole = read().unwrap();
        assert_eq!(whole.len(), models().len());
        assert!(whole.len() >= 14);
        assert!(whole.iter().all(|model| !model.id.is_empty()));
    }

    #[test]
    fn a_listing_names_each_model_by_its_id() {
        let page = json!({"data": [{"id": "a"}, {"id": ""}, {"name": "b"}, {"id": "c"}]});
        let ids: Vec<String> = listed(&page).unwrap().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["a", "c"]);
        assert_eq!(listed(&json!({})).unwrap().len(), 0);
        assert_eq!(listed(&json!({"data": null})).unwrap().len(), 0);
        assert!(listed(&json!({"data": "x"})).is_none());
    }
}
