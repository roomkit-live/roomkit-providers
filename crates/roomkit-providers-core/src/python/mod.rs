//! A value as Python writes it in text: what `repr` and `json.dumps` give.
//!
//! The words of a refusal and of a mismatch are Python's, which quote a
//! value with `repr`; the fragments a scripted provider splits a call's
//! arguments into are those of Python's `json.dumps`.

mod float;
mod json;
mod printable;
mod repr;

#[doc(hidden)]
pub use self::json::dumps;
pub(crate) use self::repr::{quoted, repr};

#[cfg(test)]
mod tests {
    /// The table of printable characters is derived from Unicode data: the
    /// notice it carries is the license file the crate ships, and the
    /// crate's license names it.
    #[test]
    fn the_crate_ships_and_declares_the_license_of_its_unicode_table() {
        let license = include_str!("../../LICENSE-UNICODE");
        let commented: String = license
            .lines()
            .map(|line| format!("//! {line}").trim_end().to_owned() + "\n")
            .collect();
        assert!(license.starts_with("UNICODE LICENSE V3\n"));
        assert!(include_str!("printable.rs").contains(&commented));
        assert_eq!(env!("CARGO_PKG_LICENSE"), "MIT AND Unicode-3.0");
    }
}
