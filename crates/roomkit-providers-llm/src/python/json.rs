//! JSON text as Python's `json.dumps` writes it by default.

use std::fmt::{self, Write};

use serde_json::Value;

use super::float::python_float;

/// `value` as Python's `json.dumps(value)` writes it: `", "` and `": "`
/// between members, every character past ASCII escaped, a float as Python's
/// `repr` writes it.
///
/// For a provider crate: a wire that carries a call's arguments as JSON text
/// in a string, as Chat Completions does, carries this text.
#[doc(hidden)]
pub fn dumps(value: &Value) -> String {
    let mut written = String::new();
    // Writing to a `String` does not fail.
    let _infallible = write_value(&mut written, value);
    written
}

/// What is left to write: a value, or the rest of a list or of an object.
/// Kept on a stack rather than in the call stack, so that a deeply nested
/// value is written whatever its depth.
enum Frame<'a> {
    Value(&'a Value),
    Array(std::slice::Iter<'a, Value>, bool),
    Object(serde_json::map::Iter<'a>, bool),
}

fn write_value(out: &mut impl Write, value: &Value) -> fmt::Result {
    let mut pending = vec![Frame::Value(value)];
    while let Some(frame) = pending.pop() {
        match frame {
            Frame::Value(value) => start(out, value, &mut pending)?,
            Frame::Array(mut items, first) => {
                if let Some(item) = items.next() {
                    if !first {
                        out.write_str(", ")?;
                    }
                    pending.push(Frame::Array(items, false));
                    pending.push(Frame::Value(item));
                } else {
                    out.write_char(']')?;
                }
            }
            Frame::Object(mut fields, first) => {
                if let Some((key, value)) = fields.next() {
                    if !first {
                        out.write_str(", ")?;
                    }
                    string(out, key)?;
                    out.write_str(": ")?;
                    pending.push(Frame::Object(fields, false));
                    pending.push(Frame::Value(value));
                } else {
                    out.write_char('}')?;
                }
            }
        }
    }
    Ok(())
}

/// Writes a scalar whole, or opens a list or an object and leaves its
/// members on `pending`.
fn start<'a>(out: &mut impl Write, value: &'a Value, pending: &mut Vec<Frame<'a>>) -> fmt::Result {
    match value {
        Value::String(text) => string(out, text),
        Value::Array(items) => {
            out.write_char('[')?;
            pending.push(Frame::Array(items.iter(), true));
            Ok(())
        }
        Value::Object(fields) => {
            out.write_char('{')?;
            pending.push(Frame::Object(fields.iter(), true));
            Ok(())
        }
        Value::Number(number) if number.is_f64() => {
            out.write_str(&python_float(number.as_f64().unwrap_or_default()))
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => out.write_str(&value.to_string()),
    }
}

fn string(out: &mut impl Write, text: &str) -> fmt::Result {
    out.write_char('"')?;
    for character in text.chars() {
        match character {
            '"' => out.write_str("\\\"")?,
            '\\' => out.write_str("\\\\")?,
            '\n' => out.write_str("\\n")?,
            '\r' => out.write_str("\\r")?,
            '\t' => out.write_str("\\t")?,
            '\u{8}' => out.write_str("\\b")?,
            '\u{c}' => out.write_str("\\f")?,
            // Python escapes a control character, a delete and every
            // character past ASCII, the ones past U+FFFF as their UTF-16
            // surrogate pair.
            c if u32::from(c) < 0x20 || !c.is_ascii() || c == '\u{7f}' => {
                let mut units = [0_u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    write!(out, "\\u{unit:04x}")?;
                }
            }
            c => out.write_char(c)?,
        }
    }
    out.write_char('"')
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // Python: json.dumps(value) of each value.
    #[test]
    fn a_value_reads_as_python_writes_it() {
        assert_eq!(
            dumps(&json!({
                "ok": true,
                "note": "already \u{2014} active \"x\"\n",
                "references": ["a.md", "b.md"],
                "emoji": "\u{1f600}",
                "none": null,
            })),
            r#"{"ok": true, "note": "already \u2014 active \"x\"\n", "references": ["a.md", "b.md"], "emoji": "\ud83d\ude00", "none": null}"#
        );
        assert_eq!(
            dumps(&json!({
                "ms": 1e-05,
                "big": 1e16,
                "n": 3,
                "neg": -0.0,
                "nested": { "a": [1, 2.5, { "b": [] }], "e": {} },
            })),
            r#"{"ms": 1e-05, "big": 1e+16, "n": 3, "neg": -0.0, "nested": {"a": [1, 2.5, {"b": []}], "e": {}}}"#
        );
    }

    // Python: json.dumps("a\x7fb\x01c\x08\x0c\t\r\\").
    #[test]
    fn a_control_character_and_a_delete_are_escaped() {
        assert_eq!(
            dumps(&json!("a\u{7f}b\u{1}c\u{8}\u{c}\t\r\\")),
            r#""a\u007fb\u0001c\b\f\t\r\\""#
        );
    }
}
