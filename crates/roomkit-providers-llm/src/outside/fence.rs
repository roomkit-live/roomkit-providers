//! Text from outside set apart in a block of its own (RFC §6.4, "Text from
//! outside in a model's context"): a tag the text cannot close.
//!
//! A tag is read as a model reads it, as `fence` of `RoomKit`'s Python
//! implementation reads it (`_text.fence`, `_lookalike`): its
//! brackets and slash in any of the forms Unicode lists as confusable with
//! them, spacing and invisible characters around them, one slash or more
//! (an escaped one included), and its name's letters in any case and form
//! (NFKC, confusables, a character that reads as several letters), with
//! invisible, control, line-break and combining characters between them,
//! and its separator as any run of dashes, underscores and spaces in any of
//! their forms, or none.

use super::tag_name::{Name, ends_closing_name, ends_opening_name, name_end};
use super::unicode;

/// `text` inside `<tag>` and `</tag>`, on lines of their own, with no tag of
/// that name a model could read in it (RFC §6.4).
///
/// Each closing tag of that name `text` holds, its end bracket there or
/// not, and each opening tag, its name ended by anything that does not
/// continue it (`<tag-list>` and `<tag.v2>` are other tags), is neutralised
/// where its name ends: an underscore follows the name, which makes it
/// another tag, and the rest is kept as written. A closing tag errs toward
/// what a model could read as one: `</tag.>` and `</tag-x>` are closing
/// tags, `</tag_x>` and `</tagx>` are not. Where the name holds a separator
/// between two letters, any run of dashes, underscores and spaces in any of
/// their forms, or none, reads as it: `</server‐instructions>`,
/// `</server instructions>` and `</serverinstructions>` close
/// `server_instructions`.
///
/// For a provider crate: a text from outside the provider writes in a
/// model's context where its wire has no place for it, a tool's result
/// beside what it refers to, is set apart as this.
#[doc(hidden)]
#[must_use]
pub fn fence(tag: &str, text: &str) -> String {
    format!("<{tag}>\n{}\n</{tag}>", without_tags(tag, text))
}

/// `text` with every closing and opening tag of `tag` neutralised, as
/// [`fence`] says, read as written, then past a combining mark of any
/// script no reader sees (U+1DC0 between a name's letters). For
/// a text about to be written as JSON inside a block, which reads it before
/// JSON hides a line break or a slash as an escape. Python's
/// `without_tags`.
fn without_tags(tag: &str, text: &str) -> String {
    let name = Name::of(tag);
    let chars: Vec<char> = text.chars().collect();
    let written: Vec<char> = underscored(&chars, &tag_ends(&chars, &name))
        .chars()
        .collect();
    let kept: Vec<usize> = (0..written.len())
        .filter(|&at| !written.get(at).is_some_and(|&c| unicode::is_hidden_mark(c)))
        .collect();
    if kept.len() == written.len() {
        return written.into_iter().collect();
    }
    let read: Vec<char> = kept
        .iter()
        .filter_map(|&at| written.get(at).copied())
        .collect();
    let ends: Vec<usize> = tag_ends(&read, &name)
        .into_iter()
        .map(|end| kept.get(end).copied().unwrap_or(written.len()))
        .collect();
    underscored(&written, &ends)
}

/// Where the name of each tag of `name` `chars` holds ends, in order, read
/// left to right without overlap.
fn tag_ends(chars: &[char], name: &Name) -> Vec<usize> {
    let mut ends = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        match tag_at(chars, at, name) {
            Some(end) => {
                ends.push(end);
                at = end;
            }
            None => at += 1,
        }
    }
    ends
}

/// `chars` with an underscore before each of `ends`, in order, one at the
/// end included: each tag's name made another tag's.
fn underscored(chars: &[char], ends: &[usize]) -> String {
    let mut body = String::with_capacity(chars.len() + ends.len());
    let mut ends = ends.iter().copied().peekable();
    for (position, c) in chars.iter().enumerate() {
        if ends.next_if_eq(&position).is_some() {
            body.push('_');
        }
        body.push(*c);
    }
    if ends.next().is_some() {
        body.push('_');
    }
    body
}

/// Where the name of a tag of `name` that opens at `chars[at]` ends: a
/// closing tag (a bracket, one slash or more, the name) or an opening one
/// (a bracket, the name), each name ended as its kind requires.
fn tag_at(chars: &[char], at: usize, name: &Name) -> Option<usize> {
    if !chars.get(at).is_some_and(|&c| reads_as(c, '<')) {
        return None;
    }
    let after_bracket = gap(chars, at + 1);
    let mut start = after_bracket;
    let mut slashes = 0;
    loop {
        let slash = start + usize::from(chars.get(start) == Some(&'\\'));
        if !chars.get(slash).is_some_and(|&c| reads_as(c, '/')) {
            break;
        }
        start = gap(chars, slash + 1);
        slashes += 1;
    }
    if slashes > 0 {
        name_end(chars, start, name, ends_closing_name)
    } else {
        name_end(chars, after_bracket, name, ends_opening_name)
    }
}

/// The position after the run of room between a tag's brackets, slash and
/// name that starts at `at`: spacing, invisible characters, combining
/// marks, a braille blank, the interlinear annotation marks.
fn gap(chars: &[char], at: usize) -> usize {
    let run = chars
        .get(at..)
        .unwrap_or_default()
        .iter()
        .take_while(|&&c| unicode::is_room(c))
        .count();
    at + run
}

/// Whether `c` reads as `sign`, a bracket or a slash: it is one, or one of
/// the forms Unicode lists as confusable with it.
fn reads_as(c: char, sign: char) -> bool {
    unicode::reads(c).any(|run| run.chars().eq([sign]))
}

#[cfg(test)]
#[path = "fence_tests.rs"]
mod tests;
