//! Ids that are no text, compared as Python compares them: a server that
//! writes a call's id as a number or as `true` names one call by it.

use serde_json::Value;

/// Whether two ids that are no text are one as Python compares them: a
/// number or `true` by its value, `true` being 1 (`1 == True`, `7 == 7.0`).
pub(super) fn same_odd(held: &Value, id: &Value) -> bool {
    match (python_number(held), python_number(id)) {
        (Some(held), Some(id)) => held.equals(id),
        _ => held == id,
    }
}

/// A number as Python holds it: a whole one exactly, else a float.
#[derive(Clone, Copy)]
enum Number {
    Int(i128),
    Float(f64),
}

impl Number {
    /// Whether the two are one number, exactly, as Python compares an
    /// integer with a float: a float equal to an integer is whole.
    fn equals(self, other: Self) -> bool {
        match (self, other) {
            (Self::Int(one), Self::Int(other)) => one == other,
            (Self::Float(one), Self::Float(other)) => one.total_cmp(&other).is_eq(),
            (Self::Int(whole), Self::Float(float)) | (Self::Float(float), Self::Int(whole)) => {
                whole_of(float) == Some(whole)
            }
        }
    }
}

/// `float` as an integer when it is a whole number an `i128` holds.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the float is whole and within i128's range, so the cast is exact"
)]
fn whole_of(float: f64) -> Option<i128> {
    // i128's bounds, as floats: 2^127 is the first float past i128::MAX.
    const BOUND: f64 = 170_141_183_460_469_231_731_687_303_715_884_105_728.0;
    (float.is_finite() && float.trunc().total_cmp(&float).is_eq() && float.abs() < BOUND)
        .then_some(float as i128)
}

/// `id` as the number Python reads it, `true` being 1.
fn python_number(id: &Value) -> Option<Number> {
    match id {
        Value::Bool(flag) => Some(Number::Int(i128::from(*flag))),
        Value::Number(number) => number
            .as_i64()
            .map(i128::from)
            .or_else(|| number.as_u64().map(i128::from))
            .map(Number::Int)
            .or_else(|| number.as_f64().map(Number::Float)),
        Value::Null | Value::String(_) | Value::Array(_) | Value::Object(_) => None,
    }
}
