//! The JSON forms of `RoomKit`'s Python models, for the types of `RoomKit`'s
//! Rust core.
//!
//! `RoomKit`'s Rust core gives its request, its answer and the events of a
//! stream no JSON form: a Rust host hands the values themselves. What
//! crosses to Python does so as JSON, in the form of `RoomKit`'s Python
//! models (`model_dump(mode="json")`), and so does what a recorded case
//! holds of a request and of its result. This crate is that conversion and
//! nothing else:
//!
//! - [`Form`]: a type read from its JSON form and written to it, for a
//!   request (`AiContext`), an answer (`AiResponse`) and an event of a
//!   stream (`StreamEvent`);
//! - [`As`]: the same through serde, for a value held in a structure that
//!   is itself read or written.
//!
//! What is read comes from `RoomKit`'s own Python models, or from a fixture
//! written from them: a form is read as it is written, a member left out
//! taking the default Python's model gives it.

#![forbid(unsafe_code)]

mod request;
mod response;
mod text_or_list;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// A type of `RoomKit`'s Rust core that has the JSON form of the Python
/// model it stands for.
pub trait Form: Sized {
    /// The value `form` writes.
    ///
    /// # Errors
    ///
    /// What the JSON reader says of a form that is not this type's.
    fn read(form: Value) -> Result<Self, serde_json::Error>;

    /// The value's JSON form, every member written, as Python's
    /// `model_dump(mode="json")` writes it.
    fn written(&self) -> Value;
}

/// A value read and written by serde in its JSON [`Form`]: for a member of
/// a structure that derives its own reading or writing.
#[derive(Clone, Debug, PartialEq)]
pub struct As<T>(pub T);

impl<T: Form> Serialize for As<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.written().serialize(serializer)
    }
}

impl<'de, T: Form> Deserialize<'de> for As<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let form = Value::deserialize(deserializer)?;
        T::read(form).map(Self).map_err(serde::de::Error::custom)
    }
}
