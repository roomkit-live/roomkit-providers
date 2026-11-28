//! A portable `WIDTHxHEIGHT` size as Gemini's named aspect ratio and
//! resolution tier, refused by the size's own name when the model cannot
//! draw it (RFC §25.2): Python's `image_geometry`.

use roomkit_ai::image::{ImageRequestError, parse_size};

use super::answer::repr;

/// The aspect ratios the Interactions image format takes across Gemini's
/// lineup (Python's `ASPECT_RATIOS`).
pub(crate) const ASPECT_RATIOS: [&str; 14] = [
    "1:1", "1:4", "1:8", "16:9", "2:3", "21:9", "3:2", "3:4", "4:1", "4:3", "4:5", "5:4", "8:1",
    "9:16",
];

/// The resolution tiers, each by the largest edge it covers (Python's
/// `SIZE_TIERS`).
const SIZE_TIERS: [(u32, &str); 4] = [(512, "512"), (1024, "1K"), (2048, "2K"), (4096, "4K")];

/// The tiers, by their names (Python's `TIERS`).
pub(crate) const TIERS: [&str; 4] = ["512", "1K", "2K", "4K"];

/// `size` as its reduced aspect ratio and the smallest tier that covers it,
/// `None` past the largest (Python's `reduce_size`).
fn reduce_size(size: &str) -> Result<(String, Option<&'static str>), ImageRequestError> {
    let (width, height) = parse_size(size)?;
    let divisor = gcd(width, height);
    let largest = width.max(height);
    let tier = SIZE_TIERS
        .iter()
        .find(|(ceiling, _)| largest <= *ceiling)
        .map(|(_, name)| *name);
    Ok((format!("{}:{}", width / divisor, height / divisor), tier))
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

/// The ratio and the tier `owner`, a model or Gemini's lineup, draws
/// `size` at (Python's `size_geometry`).
///
/// # Errors
///
/// [`ImageRequestError`], Python's words, for a size whose ratio or tier
/// `owner` does not offer.
pub(crate) fn size_geometry(
    size: &str,
    ratios: &[impl AsRef<str>],
    tiers: &[impl AsRef<str>],
    owner: &str,
) -> Result<(String, String), ImageRequestError> {
    let (aspect_ratio, tier) = reduce_size(size)?;
    if !ratios.iter().any(|ratio| ratio.as_ref() == aspect_ratio) {
        let offered = listed(ratios);
        return Err(ImageRequestError::new(format!(
            "size {} reduces to aspect ratio {aspect_ratio}, which {owner} does not offer \
             (its ratios: {offered})",
            repr(size)
        )));
    }
    let offered = tier.filter(|tier| tiers.iter().any(|offered| offered.as_ref() == *tier));
    if let Some(tier) = offered {
        return Ok((aspect_ratio, tier.to_owned()));
    }
    let needed = tier.map_or_else(
        || "a tier beyond Gemini's largest tier (4K)".to_owned(),
        |tier| format!("Gemini's {tier} tier"),
    );
    let offered = listed(tiers);
    Err(ImageRequestError::new(format!(
        "size {} needs {needed}, which {owner} does not offer (its tiers: {offered})",
        repr(size)
    )))
}

/// `names` as Python lists them in a refusal: joined by commas, `none` for
/// none.
fn listed(names: &[impl AsRef<str>]) -> String {
    if names.is_empty() {
        return "none".to_owned();
    }
    let names: Vec<&str> = names.iter().map(AsRef::as_ref).collect();
    names.join(", ")
}
