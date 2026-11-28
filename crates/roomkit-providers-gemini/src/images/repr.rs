//! Values written as Python's `repr` writes them, in the words a refusal or
//! a failure quotes them with.

use std::fmt::Write as _;

use serde_json::Value;
use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

/// `text` as Python's `repr` writes a `str`: in single quotes, or double
/// quotes when it holds a single one and no double one; its backslashes,
/// its quote and what Python does not print escaped.
pub(crate) fn repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut written = String::from(quote);
    for character in text.chars() {
        match character {
            '\\' => written.push_str("\\\\"),
            '\n' => written.push_str("\\n"),
            '\r' => written.push_str("\\r"),
            '\t' => written.push_str("\\t"),
            _ if character == quote => {
                written.push('\\');
                written.push(character);
            }
            _ if printable(character) => written.push(character),
            _ => escaped(&mut written, character),
        }
    }
    written.push(quote);
    written
}

/// Whether Python prints `character` in a `repr` as it is
/// (`str.isprintable`): no control, format, surrogate, private,
/// unassigned or separator character but the space.
fn printable(character: char) -> bool {
    if character == ' ' {
        return true;
    }
    !matches!(
        character.general_category(),
        GeneralCategory::Control
            | GeneralCategory::Format
            | GeneralCategory::Surrogate
            | GeneralCategory::PrivateUse
            | GeneralCategory::Unassigned
            | GeneralCategory::SpaceSeparator
            | GeneralCategory::LineSeparator
            | GeneralCategory::ParagraphSeparator
    )
}

/// `character` escaped as Python escapes what it does not print.
fn escaped(written: &mut String, character: char) {
    let code = u32::from(character);
    // Writing to a string cannot fail.
    if code < 0x100 {
        write!(written, "\\x{code:02x}").ok();
    } else if code < 0x1_0000 {
        write!(written, "\\u{code:04x}").ok();
    } else {
        write!(written, "\\U{code:08x}").ok();
    }
}

/// `value` as Python's `repr` writes what its JSON reader makes of it.
pub(crate) fn value_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => repr(text),
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(value_repr).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(members) => {
            let members: Vec<String> = members
                .iter()
                .map(|(name, member)| format!("{}: {}", repr(name), value_repr(member)))
                .collect();
            format!("{{{}}}", members.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // As Python 3.13 writes them.
    #[test]
    fn values_are_written_as_python_s_repr() {
        assert_eq!(repr("1024x4096\u{a0}"), "'1024x4096\\xa0'");
        assert_eq!(repr("a\u{200b}b\u{1F600}"), "'a\\u200bb\u{1F600}'");
        assert_eq!(repr("it's"), "\"it's\"");
        let status = json!({"a": [1, 2], "b": null, "c": true, "d": "it's"});
        assert_eq!(
            value_repr(&status),
            "{'a': [1, 2], 'b': None, 'c': True, 'd': \"it's\"}"
        );
    }
}
