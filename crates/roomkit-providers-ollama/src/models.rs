//! Ollama's model catalogue: Python's (`catalog/models.json`, written by
//! `scripts/generate_ollama_catalogue.py`), a snapshot of popular models for
//! their context windows; and the listing of the models a server holds as
//! Python reads it, `/api/tags`, then `/api/show` for each model's
//! capabilities.

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

/// The names of the models a page of `/api/tags` lists, as the SDK reads
/// it (`ListResponse`): each entry's `model`, an entry without one left
/// out. `None` for a page with no list of models, or an entry that is no
/// object, or whose name is no text.
pub(crate) fn installed(page: &Value) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for model in page.get("models")?.as_array()? {
        let entry = model.as_object()?;
        match entry.get("model") {
            None | Some(Value::Null) => {}
            Some(Value::String(name)) if name.is_empty() => {}
            Some(Value::String(name)) => names.push(name.clone()),
            Some(_) => return None,
        }
    }
    Some(names)
}

/// The capabilities `/api/show` answers of a model, as the SDK reads its
/// answer (`ShowResponse`): none for one it does not read, without its
/// `model_info` or with capabilities that are no list of texts.
pub(crate) fn capabilities(shown: &Value) -> Vec<String> {
    let Some(shown) = shown.as_object() else {
        return Vec::new();
    };
    if !matches!(
        shown.get("model_info"),
        Some(Value::Object(_) | Value::Null)
    ) {
        return Vec::new();
    }
    let Some(Value::Array(capabilities)) = shown.get("capabilities") else {
        return Vec::new();
    };
    let named = capabilities
        .iter()
        .map(|capability| capability.as_str().map(str::to_owned));
    named.collect::<Option<Vec<_>>>().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_catalogue_reads_whole() {
        let whole = read().unwrap();
        assert_eq!(whole.len(), models().len());
        assert!(whole.len() >= 20);
    }

    #[test]
    fn a_listing_names_each_model_and_a_show_its_capabilities() {
        let page =
            json!({"models": [{"model": "a"}, {"model": ""}, {"name": "b"}, {"model": "c"}]});
        assert_eq!(installed(&page).unwrap(), ["a", "c"]);
        assert!(installed(&json!({})).is_none());
        assert!(installed(&json!({"models": [{"model": 1}]})).is_none());
        let shown = json!({"model_info": {}, "capabilities": ["completion", "tools"]});
        assert_eq!(capabilities(&shown), ["completion", "tools"]);
        let none: [&str; 0] = [];
        assert_eq!(capabilities(&json!({"capabilities": ["completion"]})), none);
        assert_eq!(
            capabilities(&json!({"model_info": null, "capabilities": [1]})),
            none
        );
    }
}
