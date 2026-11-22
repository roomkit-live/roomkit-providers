//! The vendors the module holds, one module each. A vendor's part is its
//! configuration, how its provider is built from it and the names `RoomKit`
//! knows the provider by: what `vendor::Vendor` asks. Every entry Python
//! calls, and everything a built provider does, is written once for all of
//! them, in `vendor` and in `provider`.
//!
//! Adding a provider adds its module here, and its line in [`VENDORS`].

use super::vendor::Entries;

mod anthropic;
mod openai;

/// The entries of every vendor.
pub(crate) static VENDORS: &[Entries] = &[
    Entries::of::<anthropic::Anthropic>(),
    Entries::of::<openai::OpenAi>(),
];
