//! What every provider reads of a model catalogue the same way: the
//! catalogue its crate embeds, written from Python's by a script, the entry
//! of a model in it, the tags of that entry, and the page a model listing
//! answers.
//!
//! What a tag means is the provider's own: the catalogue of each vendor
//! names its own tags, and each provider decides what it does of one.

use roomkit_ai::ModelInfo;
use serde_json::Value;

/// What starts the tag naming the least reasoning effort a model takes.
const FLOOR_TAG: &str = "reasoning_floor_";

/// The models of `catalogue`, the text of a crate's `catalog/models.json`:
/// what it lists under `models`.
///
/// # Errors
///
/// A text that is no JSON, that lists nothing under `models`, or one of
/// whose entries does not read as the contract's.
pub fn read(catalogue: &str) -> serde_json::Result<Vec<ModelInfo>> {
    let mut whole: Value = serde_json::from_str(catalogue)?;
    let models = whole.get_mut("models").map(Value::take);
    serde_json::from_value(models.unwrap_or_default())
}

/// The entry of `model` among `models`.
#[must_use]
pub fn entry<'m>(models: &'m [ModelInfo], model: &str) -> Option<&'m ModelInfo> {
    models.iter().find(|entry| entry.id == model)
}

/// Whether `entry` carries `tag`; no entry carries none.
#[must_use]
pub fn tagged(entry: Option<&ModelInfo>, tag: &str) -> bool {
    entry.is_some_and(|entry| entry.capabilities.iter().any(|held| held == tag))
}

/// The least reasoning effort a model takes, as its `entry` declares it in
/// a tag (Python's `reasoning_floor`); `None` for an entry that declares
/// none, and for no entry.
#[must_use]
pub fn reasoning_floor(entry: Option<&ModelInfo>) -> Option<&str> {
    let mut tags = entry?.capabilities.iter();
    tags.find_map(|tag| tag.strip_prefix(FLOOR_TAG))
}

/// The models the `page` of a model listing names under `data`, each as
/// `read` makes it of its id and of the object the page holds for it;
/// `None` for a body that is no such page, one of whose models has no text
/// for an id included, which is no listing rather than an empty one.
#[must_use]
pub fn listed(page: &Value, read: impl Fn(&str, &Value) -> ModelInfo) -> Option<Vec<ModelInfo>> {
    let listed = page.get("data")?.as_array()?.iter();
    let read = |model: &Value| Some(read(model.get("id")?.as_str()?, model));
    listed.map(read).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A catalogue of two models, one tagged.
    const CATALOGUE: &str = r#"{
        "written_by": "a script",
        "models": [
            {"id": "model-a", "capabilities": ["tools", "reasoning_floor_low"]},
            {"id": "model-b"}
        ]
    }"#;

    #[test]
    fn a_catalogue_gives_its_models_and_the_entry_of_each() {
        let models = read(CATALOGUE).unwrap();
        let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(ids, ["model-a", "model-b"]);
        assert_eq!(
            entry(&models, "model-b").map(|entry| entry.id.as_str()),
            Some("model-b")
        );
        assert!(entry(&models, "model").is_none());
        for no_catalogue in ["", "[]", "{}", r#"{"models": {}}"#, r#"{"models": [7]}"#] {
            assert!(read(no_catalogue).is_err(), "{no_catalogue}");
        }
    }

    #[test]
    fn an_entry_says_its_tags_and_its_least_reasoning_effort() {
        let models = read(CATALOGUE).unwrap();
        let (tagged_one, bare) = (entry(&models, "model-a"), entry(&models, "model-b"));
        assert!(tagged(tagged_one, "tools"));
        assert!(!tagged(tagged_one, "tool") && !tagged(bare, "tools") && !tagged(None, "tools"));
        assert_eq!(reasoning_floor(tagged_one), Some("low"));
        assert_eq!((reasoning_floor(bare), reasoning_floor(None)), (None, None));
    }

    #[test]
    fn a_listing_gives_each_model_of_its_page_or_is_none() {
        let named = |id: &str, model: &Value| {
            let mut read = ModelInfo::new(id);
            read.display_name = model["name"].as_str().map(str::to_owned);
            read
        };
        let page = json!({"data": [{"id": "model-a", "name": "A"}, {"id": "model-c"}]});
        let models = listed(&page, named).unwrap();
        let read: Vec<_> = models
            .iter()
            .map(|model| (model.id.as_str(), model.display_name.as_deref()))
            .collect();
        assert_eq!(read, [("model-a", Some("A")), ("model-c", None)]);
        assert_eq!(listed(&json!({"data": []}), named), Some(Vec::new()));
        for no_page in [
            json!({}),
            json!({"data": null}),
            json!({"data": [{"name": "nameless"}]}),
            json!({"data": [{"id": "model-a"}, {"id": 7}]}),
            json!([{"id": "model-a"}]),
        ] {
            assert_eq!(listed(&no_page, named), None, "{no_page}");
        }
    }
}
