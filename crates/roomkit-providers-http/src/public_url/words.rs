//! How Python writes a text in a refusal: its `repr`.

use std::fmt::Write as _;

/// `text` as Python's `repr` of a `str` writes it, for the texts a refusal
/// names: in single quotes, double ones when it holds a single quote and no
/// double one; its backslashes and its quote escaped; a control character
/// escaped.
///
/// A character past ASCII is written as it is. Python escapes those its
/// `str.isprintable()` refuses, a no-break space among them: such a
/// character in a port's text or in a scheme's name is the one place these
/// words can differ from Python's.
pub(super) fn quoted(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut written = String::with_capacity(text.len() + 2);
    written.push(quote);
    for character in text.chars() {
        match character {
            '\\' => written.push_str("\\\\"),
            '\t' => written.push_str("\\t"),
            '\n' => written.push_str("\\n"),
            '\r' => written.push_str("\\r"),
            _ if character == quote => {
                written.push('\\');
                written.push(character);
            }
            control if u32::from(control) < 0x20 || control == '\u{7f}' => {
                // Writing to a `String` does not fail.
                let _infallible = write!(written, "\\x{:02x}", u32::from(control));
            }
            other => written.push(other),
        }
    }
    written.push(quote);
    written
}

/// `names` as Python writes a tuple of texts: `('http', 'https')`,
/// `('https',)`, `()`.
pub(super) fn tuple(names: &[&str]) -> String {
    let quoted_names: Vec<String> = names.iter().map(|name| quoted(name)).collect();
    match quoted_names.as_slice() {
        [only] => format!("({only},)"),
        several => format!("({})", several.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Python: repr() of each text, and of each tuple.
    #[test]
    fn a_text_and_a_tuple_are_written_as_python_s_repr_writes_them() {
        assert_eq!(quoted("service.example"), "'service.example'");
        assert_eq!(quoted("it's"), "\"it's\"");
        assert_eq!(quoted("say \"hi\" it's"), "'say \"hi\" it\\'s'");
        assert_eq!(quoted("a\\b\tc\u{1}\u{7f}"), "'a\\\\b\\tc\\x01\\x7f'");
        assert_eq!(tuple(&["http", "https"]), "('http', 'https')");
        assert_eq!(tuple(&["https"]), "('https',)");
        assert_eq!(tuple(&[]), "()");
    }
}
