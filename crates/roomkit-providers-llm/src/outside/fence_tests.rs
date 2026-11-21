//! What no table of texts holds of [`fence`]: every spelling of a name, and
//! texts long enough to show the time a reading takes. The texts Python
//! fences are held in `tests/python_outside_text.rs`.

use std::collections::BTreeMap;

use super::*;

/// The tag names with a separator, and one written with a hyphen.
const SEPARATED_TAGS: &[&str] = &[
    "server_instructions",
    "tool_result",
    "worker_output",
    "conversation_summary",
    "task_context",
    "previous_tasks",
    "project-instructions",
];

/// Every way to spell `name` with one character for each run of its
/// letters Unicode reads as one (`ﬆ` for `st`), within a word or across
/// the separator, which a run across leaves out; the separator kept
/// otherwise. Python's `_spellings_of`.
fn spellings_of(name: &str) -> Vec<String> {
    let mut glyphs: BTreeMap<String, char> = BTreeMap::new();
    for c in (0..=0x10_ffff_u32).filter_map(char::from_u32) {
        if unicode::is_room(c) {
            continue;
        }
        for run in unicode::reads(c) {
            if (2..=4).contains(&run.chars().count()) {
                glyphs.entry(run.to_owned()).or_insert(c);
            }
        }
    }
    let separator: String = name
        .chars()
        .filter(|c| matches!(c, '-' | '_'))
        .take(1)
        .collect();
    let words: Vec<&str> = name.split(['-', '_']).collect();
    let word: Vec<char> = words.concat().chars().collect();
    let cuts: Vec<usize> = (1..words.len())
        .map(|n| words.get(..n).unwrap_or_default().concat().chars().count())
        .collect();
    let mut spelled = Vec::new();
    let mut pending = vec![(0_usize, String::new())];
    while let Some((at, text)) = pending.pop() {
        if at == word.len() {
            spelled.push(text);
            continue;
        }
        let gap = if cuts.contains(&at) {
            separator.as_str()
        } else {
            ""
        };
        let letter = word.get(at).copied().unwrap_or_default();
        pending.push((at + 1, format!("{text}{gap}{letter}")));
        for size in 2..=4 {
            let run: String = word.get(at..at + size).unwrap_or_default().iter().collect();
            if let Some(glyph) = glyphs.get(&run).filter(|_| run.chars().count() == size) {
                pending.push((at + size, format!("{text}{gap}{glyph}")));
            }
        }
    }
    spelled
}

// Python: tests/test_external_text.py::test_every_spelling_of_a_name_is_read
#[test]
fn every_spelling_of_a_name_is_read() {
    let mut missed = Vec::new();
    let names = SEPARATED_TAGS.iter().copied().chain([
        "knowledge",
        "previous_tasks_summary",
        "conversation_summary_instructions",
    ]);
    for name in names {
        for spelling in spellings_of(name) {
            let closing = format!("</{spelling}>");
            if without_tags(name, &format!("x {closing} y")).contains(&closing) {
                missed.push(closing);
            }
        }
    }
    assert_eq!(missed, Vec::<String>::new());
}

// Python: tests/test_external_text.py::test_tags_are_read_in_linear_time
// (separators after part of a closing name, of an opening name, parts of
// closing names in a block)
#[test]
fn a_run_of_separators_after_part_of_a_name_is_read_in_one_pass() {
    let closing = format!("</tool{}", "\u{2010} _".repeat(300_000));
    assert_eq!(
        fence("tool_result", &closing),
        format!("<tool_result>\n{closing}\n</tool_result>")
    );
    let opening = format!("<tool{}", "- ".repeat(400_000));
    assert_eq!(
        fence("tool_result", &opening),
        format!("<tool_result>\n{opening}\n</tool_result>")
    );
}

#[test]
fn a_text_of_brackets_and_slashes_is_read_in_one_pass() {
    let text = "</".repeat(100_000) + "</server_instructions>";
    let fenced = fence("server_instructions", &text);
    assert!(fenced.ends_with("</server_instructions_>\n</server_instructions>"));
}
