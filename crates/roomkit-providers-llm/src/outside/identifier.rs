//! A name written outside any quote (RFC §6.4): Python's `_text.identifier`.

use super::nfc::nfc;
use super::unicode;

/// The characters an identifier keeps beside letters, their marks and
/// digits.
const EXTRA: &str = "_.@+-";

/// The most characters of an identifier.
const LIMIT: usize = 64;

/// `value` as an identifier: letters with their marks, digits and
/// `_ . @ + -`, every other character a `-` and each run of them one,
/// none at its ends, within 64 characters, composed first (NFC);
/// `fallback` when nothing is left.
///
/// A letter that reads as a colon or a quote is no letter here, nor is a
/// number that reads as punctuation. Python's `identifier` with its default
/// limit.
///
/// For a provider crate: a name the provider writes in a model's context
/// outside any quote, a tool's beside its result, is written as this.
#[doc(hidden)]
#[must_use]
pub fn identifier(value: &str, fallback: &str) -> String {
    let mut kept = String::with_capacity(value.len());
    for c in nfc(value).chars() {
        let c = if EXTRA.contains(c) || unicode::is_name_char(c) {
            c
        } else {
            '-'
        };
        if !(c == '-' && kept.ends_with('-')) {
            kept.push(c);
        }
    }
    let limited: String = kept.trim_matches('-').chars().take(LIMIT).collect();
    if limited.is_empty() {
        fallback.to_owned()
    } else {
        limited
    }
}
