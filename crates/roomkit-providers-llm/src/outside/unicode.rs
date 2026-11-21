//! What a tag and an identifier are read with, one character at a time, as
//! `RoomKit`'s Python implementation decides it (RFC §6.4): what a character
//! reads as in a tag, the room around a tag's name, its separator, what its
//! letters are read past, the marks it is read without, the characters an
//! identifier keeps and those `str.isalnum()` accepts.
//!
//! Each answer is read from a table generated from Python (`tables`), never
//! from Rust's standard library, whose Unicode version is another.

use std::cmp::Ordering;

use super::tables::classes::{ALNUM, HIDDEN_MARKS, JOINT, NAME_CHARS, ROOM, SEPARATORS};
use super::tables::look_alikes::READS;

/// Whether `ranges`, inclusive and sorted, holds `c`.
fn within(ranges: &[(char, char)], c: char) -> bool {
    ranges
        .binary_search_by(|&(start, end)| {
            if end < c {
                Ordering::Less
            } else if start > c {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        })
        .is_ok()
}

/// Whether `c` is of the room between a tag's brackets, slash and name:
/// spacing, an invisible character, a combining mark of the Latin range, a
/// braille blank, an interlinear annotation mark (Python's
/// `_lookalike.SPACE`).
pub(super) fn is_room(c: char) -> bool {
    within(ROOM, c)
}

/// Whether `c` reads as a tag name's separator, or is read past where one
/// sits: what [`is_room`] takes, a dash or an underscore in any of their
/// forms (Python's `_lookalike.SEPARATOR`).
pub(super) fn is_separator(c: char) -> bool {
    within(SEPARATORS, c)
}

/// Whether a name's letters are read past `c`: an invisible, control or
/// line-break character, a combining mark of the Latin range (Python's
/// `_lookalike.JOINT`).
pub(super) fn is_joint(c: char) -> bool {
    within(JOINT, c)
}

/// Whether `c` is a combining mark no reader sees that a tag's own reading
/// does not read past, of any script: a tag is read once more without them
/// (Python's `_lookalike.without_hidden_marks`).
pub(super) fn is_hidden_mark(c: char) -> bool {
    within(HIDDEN_MARKS, c)
}

/// Whether an identifier keeps `c`: a letter, a mark or a digit that reads
/// as one, never one that reads as a colon or a quote (Python's
/// `_text._a_name_s`).
pub(super) fn is_name_char(c: char) -> bool {
    within(NAME_CHARS, c)
}

/// Whether Python's `str.isalnum()` accepts `c`.
pub(super) fn is_alnum(c: char) -> bool {
    within(ALNUM, c)
}

/// What `c` reads as in a tag, as the classes of Python's patterns take it
/// (`_lookalike.char_class`, read without case): each letter, digit, sign
/// or run of letters it may stand for. `I` reads as `i` or `l`, `ⅵ` as
/// `vi` or `vl`, `＜` as `<`, `a` as itself; a character of no class reads
/// as nothing here.
pub(super) fn reads(c: char) -> impl Iterator<Item = &'static str> {
    READS
        .binary_search_by(|&(form, _)| form.cmp(&c))
        .ok()
        .and_then(|found| READS.get(found))
        .map_or("", |&(_, read)| read)
        .split(' ')
        .filter(|run| !run.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_answer_as_python_decides() {
        assert_eq!(
            (
                is_room('\u{a0}'),
                is_room('-'),
                is_separator('\u{2010}'),
                is_joint('\u{200b}'),
                is_hidden_mark('\u{1dc0}'),
                is_hidden_mark('\u{301}'),
                is_name_char(':'),
                is_name_char('\u{410}'),
                is_alnum('\u{e9}'),
                is_alnum('-'),
            ),
            (
                true, false, true, true, true, false, false, true, true, false
            )
        );
    }

    #[test]
    fn a_character_reads_as_each_letter_sign_or_run_it_may_stand_for() {
        let read = |c| reads(c).collect::<Vec<_>>();
        assert_eq!(read('I'), ["i", "l"]);
        assert_eq!(read('\u{2175}'), ["vi", "vl"]);
        assert_eq!(read('\u{ff1c}'), ["<"]);
        assert_eq!(read('.'), Vec::<&str>::new());
    }
}
