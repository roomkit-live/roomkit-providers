//! Constrained JSON output (RFC §6.7): the portable subset of JSON Schema a
//! response schema stays within, and what a provider fails with when it
//! cannot honour one.
//!
//! - [`check_portable_schema`]: refuses a schema outside the subset.
//! - [`schema_mismatch`] and [`SchemaMismatchOptions`]: where a document
//!   departs from a portable schema.
//! - [`ResponseSchemaError`], [`ResponseSchemaFailure`] and
//!   [`ResponseSchemaErrorOptions`]: the failure of a response schema.

mod portable;
mod response;

pub(crate) use self::portable::check_portable_object;
pub use self::portable::{SchemaMismatchOptions, check_portable_schema, schema_mismatch};
pub use self::response::{ResponseSchemaError, ResponseSchemaErrorOptions, ResponseSchemaFailure};
#[doc(hidden)]
pub use self::response::{check_schema_answer, checked_stream, schema_for_generate};
