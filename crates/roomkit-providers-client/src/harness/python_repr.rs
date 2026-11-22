//! A JSON value as Python's `repr` writes the value it reads as: what a
//! failure of Python's says of a body, which a fixture's words are checked
//! against.

use roomkit_core::rules::python_repr;
use serde_json::Value;

use super::python_float::python_float;

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
        Value::String(text) => python_repr(text),
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(repr).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(members) => {
            let members: Vec<String> = members
                .iter()
                .map(|(name, value)| format!("{}: {}", python_repr(name), repr(value)))
                .collect();
            format!("{{{}}}", members.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

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
