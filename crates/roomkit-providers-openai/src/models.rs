//! The model catalogues: Python's, one for `OpenAI` and one for each vendor
//! served on the same wire (`catalog/`, written by
//! `scripts/generate_openai_catalogue.py`), and what the provider reads of
//! a model's entry on its vendor's own endpoint.

use std::sync::LazyLock;

use roomkit_ai::ModelInfo;
use roomkit_providers_client::catalogue;
pub(crate) use roomkit_providers_client::catalogue::{reasoning_floor, tagged};
use serde_json::Value;

use crate::vendor::Vendor;

/// The tag of a model Chat Completions does not serve.
pub(crate) const RESPONSES_ONLY: &str = "responses_only";
/// The tag of a model that takes no function tools on Chat Completions.
pub(crate) const CHAT_TOOLS_REFUSED: &str = "chat_tools_refused";
/// The tag of a model that takes function tools only without reasoning.
pub(crate) const TOOLS_REASONING_NONE: &str = "tools_reasoning_none";
/// The tag of a model that keeps its reasoning effort beside function tools.
pub(crate) const TOOLS_REASONING_EFFORT: &str = "tools_reasoning_effort";

/// The embedded catalogue of each vendor, as the script wrote it.
const fn written(vendor: Vendor) -> &'static str {
    match vendor {
        Vendor::LlamaCpp { .. } | Vendor::LiteLlm => "[]",
        Vendor::OpenAi => include_str!("../catalog/models.json"),
        Vendor::DeepSeek { .. } => include_str!("../catalog/deepseek.json"),
        Vendor::Kimi => include_str!("../catalog/kimi.json"),
        Vendor::MiniMax => include_str!("../catalog/minimax.json"),
        Vendor::Xai => include_str!("../catalog/xai.json"),
        Vendor::Cerebras { .. } => include_str!("../catalog/cerebras.json"),
        Vendor::Qwen { .. } => include_str!("../catalog/qwen.json"),
        Vendor::Meta => include_str!("../catalog/meta.json"),
        Vendor::OpenRouter => include_str!("../catalog/openrouter.json"),
    }
}

/// The embedded catalogue of `vendor`, read whole.
fn read(vendor: Vendor) -> serde_json::Result<Vec<ModelInfo>> {
    catalogue::read(written(vendor))
}

/// A vendor's catalogue, read once.
type Catalogue = LazyLock<Vec<ModelInfo>>;

static OPENAI: Catalogue = LazyLock::new(|| read(Vendor::OpenAi).unwrap_or_default());
static DEEPSEEK: Catalogue = LazyLock::new(|| read(DEEPSEEK_VENDOR).unwrap_or_default());
static KIMI: Catalogue = LazyLock::new(|| read(Vendor::Kimi).unwrap_or_default());
static MINIMAX: Catalogue = LazyLock::new(|| read(Vendor::MiniMax).unwrap_or_default());
static XAI: Catalogue = LazyLock::new(|| read(Vendor::Xai).unwrap_or_default());
static CEREBRAS: Catalogue = LazyLock::new(|| read(CEREBRAS_VENDOR).unwrap_or_default());
static QWEN: Catalogue = LazyLock::new(|| read(QWEN_VENDOR).unwrap_or_default());
static META: Catalogue = LazyLock::new(|| read(Vendor::Meta).unwrap_or_default());
static OPENROUTER: Catalogue = LazyLock::new(|| read(Vendor::OpenRouter).unwrap_or_default());

/// DeepSeek, whatever its configuration states: its catalogue is one.
const DEEPSEEK_VENDOR: Vendor = Vendor::DeepSeek {
    enable_thinking: None,
};

/// Cerebras, whatever its configuration states.
const CEREBRAS_VENDOR: Vendor = Vendor::Cerebras {
    reasoning_format: None,
    clear_thinking: None,
};

/// Qwen, whatever its configuration states.
const QWEN_VENDOR: Vendor = Vendor::Qwen {
    enable_thinking: None,
};

/// The curated catalogue of `vendor`, in Python's order.
pub(crate) fn of(vendor: Vendor) -> &'static [ModelInfo] {
    match vendor {
        Vendor::LlamaCpp { .. } | Vendor::LiteLlm => &[],
        Vendor::OpenAi => &OPENAI,
        Vendor::DeepSeek { .. } => &DEEPSEEK,
        Vendor::Kimi => &KIMI,
        Vendor::MiniMax => &MINIMAX,
        Vendor::Xai => &XAI,
        Vendor::Cerebras { .. } => &CEREBRAS,
        Vendor::Qwen { .. } => &QWEN,
        Vendor::Meta => &META,
        Vendor::OpenRouter => &OPENROUTER,
    }
}

/// `OpenAI`'s curated catalogue.
pub(crate) fn models() -> &'static [ModelInfo] {
    of(Vendor::OpenAi)
}

/// The entry of `OpenAI`'s catalogue for `model`.
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
/// catalogue does not carry (Python's `_VISION_PREFIXES`), which DeepSeek's
/// provider inherits.
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

/// Whether `model`, which its vendor's catalogue does not carry, takes
/// images, by how its name starts.
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
        let whole = read(Vendor::OpenAi).unwrap();
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

    // An empty catalogue would quietly lift a vendor's floors and what it
    // says of images: each embedded copy reads whole.
    #[test]
    fn each_vendor_s_catalogue_reads_whole_with_its_floors() {
        let floor = |vendor: Vendor, model: &str| {
            assert_eq!(read(vendor).unwrap().len(), of(vendor).len());
            reasoning_floor(catalogue::entry(of(vendor), model))
        };
        assert_eq!(floor(DEEPSEEK_VENDOR, "deepseek-v4-pro"), None);
        assert_eq!(floor(Vendor::Kimi, "kimi-k3"), Some("low"));
        assert_eq!(floor(Vendor::MiniMax, "MiniMax-M3.1-Flash"), Some("low"));
        assert_eq!(floor(Vendor::MiniMax, "MiniMax-M3"), Some("none"));
    }
}
