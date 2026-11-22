//! `OpenAI`'s model catalogue: Python's (`catalog/models.json`, written by
//! `scripts/generate_openai_catalogue.py`), and what the provider reads of
//! a model's entry on `OpenAI`'s own endpoint.

use std::sync::LazyLock;

use roomkit_ai::ModelInfo;
use roomkit_providers_client::catalogue;
pub(crate) use roomkit_providers_client::catalogue::{reasoning_floor, tagged};
use serde_json::Value;

const CATALOGUE: &str = include_str!("../catalog/models.json");

/// The tag of a model Chat Completions does not serve.
pub(crate) const RESPONSES_ONLY: &str = "responses_only";
/// The tag of a model that takes no function tools on Chat Completions.
pub(crate) const CHAT_TOOLS_REFUSED: &str = "chat_tools_refused";
/// The tag of a model that takes function tools only without reasoning.
pub(crate) const TOOLS_REASONING_NONE: &str = "tools_reasoning_none";
/// The tag of a model that keeps its reasoning effort beside function tools.
pub(crate) const TOOLS_REASONING_EFFORT: &str = "tools_reasoning_effort";

static MODELS: LazyLock<Vec<ModelInfo>> = LazyLock::new(|| read().unwrap_or_default());

/// The embedded catalogue, read whole.
fn read() -> serde_json::Result<Vec<ModelInfo>> {
    catalogue::read(CATALOGUE)
}

/// `OpenAI`'s curated catalogue.
pub(crate) fn models() -> &'static [ModelInfo] {
    &MODELS
}

/// The catalogue's entry for `model`.
pub(crate) fn entry(model: &str) -> Option<&'static ModelInfo> {
    catalogue::entry(models(), model)
}

/// The models the `page` a `GET /models` answered lists, each read by its
/// id alone, as Python's `list_models` reads it; `None` for a body that is
/// no such page, which is no listing rather than an empty one.
pub(crate) fn listed(page: &[u8]) -> Option<Vec<ModelInfo>> {
    let page: Value = serde_json::from_slice(crate::failure::past_mark(page)).ok()?;
    catalogue::listed(&page, |id, _| ModelInfo::new(id))
}

/// How the names of the models that take images start, for a model the
/// catalogue does not carry (Python's `_VISION_PREFIXES`).
const VISION_PREFIXES: [&str; 8] = [
    "gpt-5",
    "gpt-4o",
    "gpt-4.1",
    "gpt-4-turbo",
    "gpt-4-vision",
    "o1",
    "o3",
    "o4",
];

/// Whether `model`, which the catalogue does not carry, takes images on
/// `OpenAI`'s own endpoint, by how its name starts.
pub(crate) fn sees_images_by_its_name(model: &str) -> bool {
    VISION_PREFIXES
        .iter()
        .any(|prefix| model.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_reads_whole_and_names_what_the_provider_reads() {
        let whole = read().unwrap();
        assert_eq!(whole.len(), models().len());
        assert!(whole.len() >= 26);
        let entry = |model: &str| entry(model);
        assert!(tagged(entry("gpt-5.5-pro"), RESPONSES_ONLY));
        assert!(tagged(entry("gpt-6-astra"), CHAT_TOOLS_REFUSED));
        assert!(tagged(entry("gpt-5.4"), TOOLS_REASONING_NONE));
        assert!(tagged(entry("gpt-5.1"), TOOLS_REASONING_EFFORT));
        assert!(!tagged(entry("gpt-4o"), TOOLS_REASONING_EFFORT));
        assert!(!tagged(None, RESPONSES_ONLY));
        assert_eq!(reasoning_floor(entry("gpt-5")), Some("minimal"));
        assert_eq!(reasoning_floor(entry("gpt-5.1")), Some("none"));
        assert_eq!(reasoning_floor(entry("o3")), Some("low"));
        assert_eq!(reasoning_floor(entry("gpt-4o")), None);
        assert_eq!(reasoning_floor(entry("no-such-model")), None);
    }
}
