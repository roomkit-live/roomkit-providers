//! A value as Python's `repr` writes it.

use std::cmp::Ordering;
use std::fmt::Write as _;

use serde_json::Value;

use super::float::python_float;
use super::printable::PRINTABLE;

/// `value` as Python's `repr` writes the value it reads as: `None`, `True`,
/// a text in quotes, a list in brackets and a mapping in braces.
pub(crate) fn repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) if number.is_f64() => {
            number.as_f64().map_or_else(String::new, python_float)
        }
        Value::Number(number) => number.to_string(),
        Value::String(text) => repr_text(text),
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(repr).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(members) => {
            let members: Vec<String> = members
                .iter()
                .map(|(name, value)| format!("{}: {}", repr_text(name), repr(value)))
                .collect();
            format!("{{{}}}", members.join(", "))
        }
    }
}

/// `text` as Python's `repr` of a `str` writes it: in single quotes, double
/// ones when it holds a single quote and no double one; its backslashes and
/// its quote escaped, `\t`, `\n` and `\r` as such, and every other character
/// `str.isprintable()` refuses as `\xNN` under U+0100, `\uNNNN` under
/// U+10000, `\UNNNNNNNN` past it (a no-break space as `\xa0`, a
/// right-to-left override as `\u202e`).
fn repr_text(text: &str) -> String {
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
            _ if is_printable(character) => written.push(character),
            _ => {
                let code = u32::from(character);
                // Writing to a `String` does not fail.
                let _infallible = if code < 0x100 {
                    write!(written, "\\x{code:02x}")
                } else if code < 0x1_0000 {
                    write!(written, "\\u{code:04x}")
                } else {
                    write!(written, "\\U{code:08x}")
                };
            }
        }
    }
    written.push(quote);
    written
}

/// Whether Python's `str.isprintable()` accepts `c`: not a control, a
/// format, a separator other than the space, a private-use, a surrogate or
/// an unassigned character.
fn is_printable(c: char) -> bool {
    PRINTABLE
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Values Python 3.13 gives (`print(repr(...))`).
    #[test]
    fn a_text_is_written_as_python_s_repr_writes_it() {
        let cases = [
            ("Bob\u{a0}Smith", "'Bob\\xa0Smith'"),
            ("a\u{200b}b", "'a\\u200bb'"),
            ("x\u{202e}y", "'x\\u202ey'"),
            ("l1\u{2028}l2", "'l1\\u2028l2'"),
            ("c1\u{85}", "'c1\\x85'"),
            ("\u{ad}", "'\\xad'"),
            ("pu\u{f0000}", "'pu\\U000f0000'"),
            ("t\u{e0001}", "'t\\U000e0001'"),
            (
                "caf\u{e9} \u{4e2d} \u{1f600}",
                "'caf\u{e9} \u{4e2d} \u{1f600}'",
            ),
            ("it's", "\"it's\""),
            ("say \"hi\" it's", "'say \"hi\" it\\'s'"),
            ("\u{0}\u{1f}\u{7f}\\", "'\\x00\\x1f\\x7f\\\\'"),
            ("tab\tline\nend\r", "'tab\\tline\\nend\\r'"),
        ];
        let written: Vec<String> = cases.iter().map(|(text, _)| repr_text(text)).collect();
        let expected: Vec<String> = cases.iter().map(|(_, repr)| (*repr).to_owned()).collect();
        assert_eq!(written, expected);
    }

    // Python: repr([None, True, False, 3, 2.5, 1e16, "it's",
    // {"a": ["x", {"b": None}]}, []]).
    #[test]
    fn a_value_is_written_as_python_s_repr_writes_it() {
        let value =
            json!([null, true, false, 3, 2.5, 1e16, "it's", { "a": ["x", { "b": null }] }, []]);
        assert_eq!(
            repr(&value),
            r#"[None, True, False, 3, 2.5, 1e+16, "it's", {'a': ['x', {'b': None}]}, []]"#
        );
    }
}
