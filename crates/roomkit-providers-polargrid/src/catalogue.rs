//! PolarGrid's catalogue: Python's (`catalog/models.json`, written by
//! `scripts/generate_polargrid_catalogue.py`). The chat models it offers;
//! every model it knows, a pilot and a speech model among them, which a
//! listing backfills from; the edges and their friendly names, which a
//! configuration pins one by; and the edges polargrid-sdk knows, the URL a
//! pinned region is sent to and the one it falls back to.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use roomkit_ai::ModelInfo;
use serde::{Deserialize, Serialize};

const CATALOGUE: &str = include_str!("../catalog/models.json");

/// A PolarGrid edge: its id, its name and where it is (Python's
/// `PolarGridRegion`). An edge in Canada has a location that starts with
/// `Canada`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolarGridRegion {
    /// The edge's id (`yul-01`).
    pub id: Option<String>,
    /// Its name (`Montreal`).
    pub name: Option<String>,
    /// Where it is (`Canada East`).
    pub location: Option<String>,
}

/// An edge as polargrid-sdk knows it.
#[derive(Debug, Deserialize)]
pub(crate) struct Edge {
    pub(crate) url: String,
    pub(crate) name: String,
}

#[derive(Debug, Deserialize)]
struct Catalogue {
    models: Vec<ModelInfo>,
    curated: Vec<ModelInfo>,
    regions: Vec<PolarGridRegion>,
    aliases: BTreeMap<String, String>,
    edges: BTreeMap<String, Edge>,
    fallback: String,
}

static READ: LazyLock<Catalogue> = LazyLock::new(|| {
    serde_json::from_str(CATALOGUE).unwrap_or_else(|_| Catalogue {
        models: Vec::new(),
        curated: Vec::new(),
        regions: Vec::new(),
        aliases: BTreeMap::new(),
        edges: BTreeMap::new(),
        fallback: String::new(),
    })
});

/// The chat models the catalogue offers.
pub(crate) fn models() -> &'static [ModelInfo] {
    &READ.models
}

/// Every model the catalogue knows: what a listing backfills from.
pub(crate) fn curated() -> &'static [ModelInfo] {
    &READ.curated
}

/// The entry of `model` among every model the catalogue knows.
pub(crate) fn entry(model: &str) -> Option<&'static ModelInfo> {
    READ.curated.iter().find(|known| known.id == model)
}

/// The edges, as Python's `available_regions` lists them.
pub(crate) fn regions() -> &'static [PolarGridRegion] {
    &READ.regions
}

/// The edge id `region` stands for, by its id or its friendly name, in any
/// case; `None` for one it names no edge of (Python's `resolve_region_id`).
pub(crate) fn region_id(region: &str) -> Option<String> {
    let lowered = region.to_lowercase();
    let resolved = READ.aliases.get(&lowered).cloned().unwrap_or(lowered);
    let known = READ
        .regions
        .iter()
        .any(|known| known.id.as_deref() == Some(&resolved));
    known.then_some(resolved)
}

/// The edge a pinned `region` is sent to, as polargrid-sdk resolves it: a
/// friendly name for its id, anything else as it is written, case kept;
/// the URL and the name of the edge the SDK knows by it, else the edge's
/// URL made of it, with no name.
pub(crate) fn pinned(region: &str) -> (String, String, Option<String>) {
    let resolved = READ.aliases.get(&region.to_lowercase()).cloned();
    let resolved = resolved.unwrap_or_else(|| region.to_owned());
    if let Some(edge) = READ.edges.get(&resolved) {
        return (edge.url.clone(), resolved, Some(edge.name.clone()));
    }
    let url = format!("https://api.{resolved}.edge.polargrid.ai");
    (url, resolved, None)
}

/// The edge polargrid-sdk falls back to when the autorouter picks none:
/// its URL, its id and its name.
pub(crate) fn fallback() -> (String, String, Option<String>) {
    let id = READ.fallback.clone();
    let edge = READ.edges.get(&id);
    let url = edge.map(|edge| edge.url.clone()).unwrap_or_default();
    (url, id, edge.map(|edge| edge.name.clone()))
}

/// The region of the catalogue whose id is `id`.
pub(crate) fn region(id: &str) -> Option<&'static PolarGridRegion> {
    READ.regions
        .iter()
        .find(|known| known.id.as_deref() == Some(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_reads_whole() {
        assert_ne!(models().len(), 0);
        assert!(curated().len() >= 6);
        assert_eq!(regions().len(), 16);
        assert_eq!(fallback().1, "yto-01");
    }

    // Python: roomkit.providers.polargrid.models.resolve_region_id
    #[test]
    fn a_region_reads_by_its_id_or_its_name_in_any_case() {
        assert_eq!(region_id("yul-01").as_deref(), Some("yul-01"));
        assert_eq!(region_id("YTO-01").as_deref(), Some("yto-01"));
        assert_eq!(region_id("Montreal").as_deref(), Some("yul-01"));
        assert_eq!(region_id("sf").as_deref(), Some("sfo-01"));
        assert_eq!(region_id("yul-2"), None);
        // polargrid-sdk: REGION_ALIASES.get(region.lower(), region)
        for toronto in ["toronto", "Toronto", "YTO"] {
            let (url, id, name) = pinned(toronto);
            assert_eq!(url, "https://api.yto-01.edge.polargrid.ai");
            assert_eq!((id.as_str(), name.as_deref()), ("yto-01", Some("Toronto")));
        }
        let (url, id, name) = pinned("YTO-01");
        assert_eq!(
            (url.as_str(), id.as_str()),
            ("https://api.YTO-01.edge.polargrid.ai", "YTO-01")
        );
        assert_eq!(name, None);
    }
}
