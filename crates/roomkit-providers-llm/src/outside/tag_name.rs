//! A tag's name as a model reads it (RFC §6.4): its letters in any case and
//! form, a character that reads as several of them, what is read past
//! between them, and its separator in any form or none. Python's
//! `_lookalike.name_pattern`.

use super::unicode;

/// A tag's name as `fence` reads it: its characters in lower case, the
/// separators between two of its letters left out, and before which of
/// them such a separator sat.
pub(super) struct Name {
    letters: Vec<char>,
    breaks: Vec<bool>,
}

impl Name {
    /// `tag`'s name: each run of separators (a dash, an underscore, a
    /// space, in any of their forms) between two of its letters left out
    /// and marked before the letter after it; one at its start or end kept
    /// as a letter, read as written.
    pub(super) fn of(tag: &str) -> Self {
        let written: Vec<char> = tag.chars().flat_map(char::to_lowercase).collect();
        let first = written.iter().position(|&c| !unicode::is_separator(c));
        let last = written.iter().rposition(|&c| !unicode::is_separator(c));
        let mut name = Self {
            letters: Vec::with_capacity(written.len()),
            breaks: Vec::with_capacity(written.len()),
        };
        let mut separated = false;
        for (index, &c) in written.iter().enumerate() {
            let inside =
                first.is_some_and(|first| index > first) && last.is_some_and(|last| index < last);
            if inside && unicode::is_separator(c) {
                separated = true;
                continue;
            }
            name.letters.push(c);
            name.breaks.push(std::mem::take(&mut separated));
        }
        name
    }

    /// Whether `run` spells the letters from `position`: across a
    /// separator, which it then leaves out (`ﬆ` for `s_t` in
    /// `previous_tasks`), too.
    fn spells(&self, position: usize, run: &str) -> bool {
        let end = position + run.chars().count();
        self.letters
            .get(position..end)
            .is_some_and(|letters| letters.iter().copied().eq(run.chars()))
    }

    /// Whether a separator sat before the letter at `position`.
    fn breaks_before(&self, position: usize) -> bool {
        self.breaks.get(position).copied().unwrap_or(false)
    }
}

/// The position after the last letter of `name` spelled from `chars[start]`,
/// when what follows lets it end (`ends`): each character read as any run
/// of the name's letters it may stand for, an invisible, control,
/// line-break or combining character read past between them, and where
/// the name holds a separator, any run of separators or none. The
/// spellings are read as an automaton whose states are the positions in
/// `name` reached.
pub(super) fn name_end(
    chars: &[char],
    start: usize,
    name: &Name,
    ends: fn(Option<char>) -> bool,
) -> Option<usize> {
    let mut reached = vec![0];
    let mut next = Vec::new();
    for (at, &c) in chars.iter().enumerate().skip(start) {
        if at > start && unicode::is_joint(c) {
            continue;
        }
        next.clear();
        for &position in &reached {
            if name.breaks_before(position) && unicode::is_separator(c) && !next.contains(&position)
            {
                next.push(position);
            }
            for run in readings(c) {
                if !name.spells(position, &run) {
                    continue;
                }
                let end = position + run.chars().count();
                if end == name.letters.len() {
                    if ends(chars.get(at + 1).copied()) {
                        return Some(at + 1);
                    }
                } else if !next.contains(&end) {
                    next.push(end);
                }
            }
        }
        if next.is_empty() {
            return None;
        }
        std::mem::swap(&mut reached, &mut next);
    }
    None
}

/// The runs of a tag name's characters `c` may read as: what the look-alike
/// classes read it as, a letter, a digit, a sign or a run of letters, and
/// `c` itself in lower case for any other character a name holds (`-`, `.`).
fn readings(c: char) -> Vec<String> {
    let mut runs: Vec<String> = unicode::reads(c).map(str::to_owned).collect();
    let written: String = c.to_lowercase().collect();
    if !runs.contains(&written) {
        runs.push(written);
    }
    runs
}

/// Whether a closing tag's name may end before `next`: anything but a
/// letter, a digit or an underscore of the ASCII range, as a case-blind
/// match reads them (`ſ` for `s`, the Kelvin sign for `k`), continues it.
pub(super) fn ends_closing_name(next: Option<char>) -> bool {
    !next.is_some_and(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, '_' | '\u{17f}' | '\u{212a}' | '\u{130}' | '\u{131}')
    })
}

/// Whether an opening tag's name may end before `next`: a word character,
/// a dot, a colon or a hyphen continues it.
pub(super) fn ends_opening_name(next: Option<char>) -> bool {
    !next.is_some_and(|c| unicode::is_alnum(c) || matches!(c, '_' | '.' | ':' | '-'))
}
