//! Reading JSON as narrowly as Python's models read it: a model from an
//! object and from nothing else, a name and a `type` from a string and from
//! nothing else.
//!
//! The readers serde derives take more: a list of a model's values for the
//! model, a one-key object for a name, and, from some sources, a number for
//! a `type`. Every type of the contract that has a JSON form is read through
//! this module instead.
//!
//! - [`ObjectOnly`] and [`read_as_object!`]: a model, read from an object.
//! - [`read_name`]: a name, read from a string.
//! - [`Tagged`] and [`read_tagged`]: a union of models told apart by their
//!   `type`.

mod name;
mod object;
mod tagged;

pub(crate) use self::name::read_name;
pub(crate) use self::object::{ObjectOnly, read_as_object};
pub(crate) use self::tagged::{Tagged, read_tagged};
