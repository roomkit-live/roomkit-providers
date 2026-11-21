//! Canonical composition (NFC) as Python's `unicodedata.normalize` gives it
//! (Unicode Standard Annex #15), from the tables of Python's Unicode data
//! rather than the Unicode version of Rust's standard library.

use super::tables::normalization::{COMBINING_CLASSES, COMPOSITIONS, DECOMPOSITIONS};

/// The canonical combining class of `c`, 0 for a starter.
fn combining_class(c: char) -> u8 {
    COMBINING_CLASSES
        .binary_search_by(|&(mark, _)| mark.cmp(&c))
        .ok()
        .and_then(|found| COMBINING_CLASSES.get(found))
        .map_or(0, |&(_, class)| class)
}

/// The first and last Hangul syllables, the leading and vowel jamo, and the
/// counts the syllables are composed from (Unicode §3.12).
const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;

/// `c`'s full canonical decomposition, pushed onto `out`.
fn decompose_into(c: char, out: &mut Vec<char>) {
    let code = u32::from(c);
    if let Some(index) = code.checked_sub(S_BASE).filter(|index| *index < S_COUNT) {
        let jamo = [
            Some(L_BASE + index / N_COUNT),
            Some(V_BASE + (index % N_COUNT) / T_COUNT),
            Some(index % T_COUNT).filter(|t| *t > 0).map(|t| T_BASE + t),
        ];
        out.extend(jamo.into_iter().flatten().filter_map(char::from_u32));
        return;
    }
    match DECOMPOSITIONS.binary_search_by(|&(composite, _)| composite.cmp(&c)) {
        Ok(found) => {
            if let Some(&(_, decomposed)) = DECOMPOSITIONS.get(found) {
                out.extend(decomposed.chars());
            }
        }
        Err(_) => out.push(c),
    }
}

/// What `first` and `second` compose into, if they are a primary composite
/// or a Hangul syllable's parts.
fn compose(first: char, second: char) -> Option<char> {
    let (starter, next) = (u32::from(first), u32::from(second));
    let leading = starter.wrapping_sub(L_BASE);
    let vowel = next.wrapping_sub(V_BASE);
    if leading < L_COUNT && vowel < V_COUNT {
        return char::from_u32(S_BASE + (leading * V_COUNT + vowel) * T_COUNT);
    }
    let syllable = starter.wrapping_sub(S_BASE);
    let trailing = next.wrapping_sub(T_BASE);
    if syllable < S_COUNT && syllable % T_COUNT == 0 && trailing > 0 && trailing < T_COUNT {
        return char::from_u32(starter + trailing);
    }
    COMPOSITIONS
        .binary_search_by(|&(x, y, _)| (x, y).cmp(&(first, second)))
        .ok()
        .and_then(|found| COMPOSITIONS.get(found))
        .map(|&(_, _, composite)| composite)
}

/// `text` in Normalization Form C: decomposed, its marks put in canonical
/// order, then composed again (Unicode Standard Annex #15).
pub(super) fn nfc(text: &str) -> String {
    let mut chars = Vec::with_capacity(text.len());
    for c in text.chars() {
        decompose_into(c, &mut chars);
    }
    canonical_order(&mut chars);
    compose_all(&chars).into_iter().collect()
}

/// Sorts each run of combining marks by their class, stably.
fn canonical_order(chars: &mut [char]) {
    let mut start = 0;
    while start < chars.len() {
        let run = chars
            .get(start..)
            .unwrap_or_default()
            .iter()
            .take_while(|c| combining_class(**c) > 0)
            .count();
        if let Some(marks) = chars.get_mut(start..start + run) {
            marks.sort_by_key(|c| combining_class(*c));
        }
        start += run.max(1);
    }
}

/// The canonical composition of `chars`, decomposed and in canonical order:
/// each character joins the last starter when nothing between them blocks
/// it, a mark of the same class or higher, and the pair composes.
fn compose_all(chars: &[char]) -> Vec<char> {
    let mut composed: Vec<char> = Vec::with_capacity(chars.len());
    // The last starter, by its position in `composed`, and the class of the
    // last character kept since it, `None` when there is none.
    let mut starter: Option<usize> = None;
    let mut since: Option<u8> = None;
    for &c in chars {
        let class = combining_class(c);
        let blocked = since.is_some_and(|last| last == 0 || last >= class);
        if !blocked && let Some(at) = starter {
            let made = composed.get(at).and_then(|&first| compose(first, c));
            if let (Some(made), Some(slot)) = (made, composed.get_mut(at)) {
                *slot = made;
                continue;
            }
        }
        if class == 0 {
            starter = Some(composed.len());
            since = None;
        } else {
            since = Some(class);
        }
        composed.push(c);
    }
    composed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values Python 3.13 gives (`unicodedata.normalize("NFC", ...)`).
    #[test]
    fn nfc_composes_as_python_does() {
        let cases = [
            ("e\u{301}", "\u{e9}"),
            ("\u{438}\u{306}", "\u{439}"),
            ("\u{1100}\u{1161}\u{11a8}", "\u{ac01}"),
            ("a\u{323}\u{302}", "\u{1ead}"),
            ("a\u{302}\u{323}", "\u{1ead}"),
            ("\u{2126}", "\u{3a9}"),
            ("A\u{30a}\u{301}", "\u{1fa}"),
            ("\u{e9}", "\u{e9}"),
            ("q\u{307}\u{323}", "q\u{323}\u{307}"),
            ("Alice", "Alice"),
        ];
        let normalized: Vec<String> = cases.iter().map(|(text, _)| nfc(text)).collect();
        let expected: Vec<String> = cases.iter().map(|(_, nfc)| (*nfc).to_owned()).collect();
        assert_eq!(normalized, expected);
    }
}
