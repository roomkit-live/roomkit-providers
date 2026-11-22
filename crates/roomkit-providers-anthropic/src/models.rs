//! Anthropic's model catalogue: Python's (`catalog/models.json`, written by
//! `scripts/generate_anthropic_catalogue.py`), and what the provider reads
//! of a model's entry, whatever the endpoint that serves the model.

use std::sync::LazyLock;

use roomkit_ai::ModelInfo;
use roomkit_providers_client::catalogue;
pub(crate) use roomkit_providers_client::catalogue::{reasoning_floor, tagged};
use serde_json::Value;

const CATALOGUE: &str = include_str!("../catalog/models.json");

/// The tag of a model that can hold a tool declared and unseen.
pub(crate) const DEFERRED_TOOLS: &str = "deferred_tools";
/// The tag of a model that refuses `output_config.effort`.
pub(crate) const EFFORT_REFUSED: &str = "effort_refused";
/// The tag of a model that refuses the effort `xhigh` and takes the others.
pub(crate) const EFFORT_NO_XHIGH: &str = "effort_no_xhigh";
/// The tag of a model that takes the efforts `low`, `medium` and `high`.
pub(crate) const EFFORT_UP_TO_HIGH: &str = "effort_up_to_high";
/// The tag of a model whose reasoning is turned off by `disabled`.
pub(crate) const THINKING_OFF_DISABLED: &str = "thinking_off_disabled";
/// The tag of a model whose reasoning is turned off by `between_tools`.
pub(crate) const THINKING_OFF_BETWEEN_TOOLS: &str = "thinking_off_between_tools";
/// The tag of a model that takes no effort above `high` with reasoning off.
pub(crate) const THINKING_OFF_EFFORT_HIGH: &str = "thinking_off_effort_high";
/// How the name of a model that takes images starts, for a model the
/// catalogue does not carry (Python's `_VISION_PREFIXES`): every Claude
/// model since Claude 3 takes them.
const VISION_PREFIX: &str = "claude-";

static MODELS: LazyLock<Vec<ModelInfo>> = LazyLock::new(|| read().unwrap_or_default());

/// The embedded catalogue, read whole.
fn read() -> serde_json::Result<Vec<ModelInfo>> {
    catalogue::read(CATALOGUE)
}

/// Anthropic's curated catalogue.
pub(crate) fn models() -> &'static [ModelInfo] {
    &MODELS
}

/// The catalogue's entry for `model`.
pub(crate) fn entry(model: &str) -> Option<&'static ModelInfo> {
    catalogue::entry(models(), model)
}

/// Whether `model` takes images, as Python's provider answers it: what the
/// catalogue says of it, else by how its name starts.
pub(crate) fn sees_images(model: &str) -> bool {
    let stated = entry(model).and_then(|entry| entry.supports_vision);
    stated.unwrap_or_else(|| model.starts_with(VISION_PREFIX))
}

/// The models the `page` a `GET /v1/models` answered lists, each read by
/// its id and the name its endpoint gives it, as Python's `list_models`
/// reads the page; `None` for a body that is no such page, which is no
/// listing rather than an empty one.
pub(crate) fn listed(page: &Value) -> Option<Vec<ModelInfo>> {
    catalogue::listed(page, |id, model| {
        let mut read = ModelInfo::new(id);
        let name = model.get("display_name").and_then(Value::as_str);
        read.display_name = name.map(str::to_owned);
        read
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_catalogue_reads_whole_and_names_what_the_provider_reads() {
        let whole = read().unwrap();
        assert_eq!(whole.len(), models().len());
        assert!(whole.len() >= 26);
        assert!(whole.iter().all(|model| !model.id.is_empty()));
        assert!(tagged(entry("claude-opus-5-5"), DEFERRED_TOOLS));
        assert!(tagged(entry("claude-sonnet-4-5"), EFFORT_REFUSED));
        assert!(tagged(entry("claude-sonnet-4-6"), EFFORT_NO_XHIGH));
        assert!(tagged(entry("claude-opus-4-5"), EFFORT_UP_TO_HIGH));
        assert!(tagged(entry("claude-opus-5"), THINKING_OFF_DISABLED));
        assert!(tagged(
            entry("claude-sonnet-5-5"),
            THINKING_OFF_BETWEEN_TOOLS
        ));
        assert!(tagged(entry("claude-opus-5"), THINKING_OFF_EFFORT_HIGH));
        assert!(!tagged(entry("claude-opus-4-1"), DEFERRED_TOOLS));
        assert!(!tagged(None, DEFERRED_TOOLS));
        assert_eq!(reasoning_floor(entry("claude-opus-5-5")), Some("low"));
        assert_eq!(reasoning_floor(entry("claude-opus-5")), None);
        assert_eq!(reasoning_floor(entry("no-such-model")), None);
    }

    // Python: src/roomkit/providers/anthropic/ai.py::AnthropicAIProvider.supports_vision
    #[test]
    fn a_model_sees_images_by_the_catalogue_else_by_its_name() {
        assert!(sees_images("claude-opus-5-5"));
        assert!(sees_images("claude-next-9"));
        assert!(!sees_images("local-model"));
        assert!(!sees_images("my-claude-opus"));
    }

    #[test]
    fn a_listing_reads_ids_and_names_and_nothing_else() {
        let page = json!({"data": [
            {"id": "claude-next-1", "display_name": "Claude Next 1", "max_input_tokens": 9},
            {"id": "local-model"},
        ]});
        let listed = listed(&page).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].display_name.as_deref(), Some("Claude Next 1"));
        assert_eq!(
            (listed[0].context_window, listed[1].display_name.as_deref()),
            (None, None)
        );
        for no_page in [
            json!({}),
            json!({"data": null}),
            json!({"data": [{"display_name": "Nameless"}]}),
            json!({"data": [{"id": 7}]}),
            json!([{"id": "claude-next-1"}]),
        ] {
            assert_eq!(super::listed(&no_page), None, "{no_page}");
        }
    }
}
