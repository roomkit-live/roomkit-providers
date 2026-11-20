//! A value as Python writes it in text: what `repr` and `json.dumps` give.
//!
//! The words of a refusal and of a mismatch are Python's, which quote a
//! value with `repr`; the fragments a scripted provider splits a call's
//! arguments into are those of Python's `json.dumps`.

mod float;
mod json;
mod printable;
mod repr;

pub(crate) use self::json::dumps;
pub(crate) use self::repr::repr;
