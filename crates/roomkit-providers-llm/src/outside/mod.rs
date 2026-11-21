//! Text a model reads that neither the host nor the provider wrote, set
//! apart from theirs (RFC §6.4, "Text from outside in a model's context"):
//! what a tool returned, fenced in a block it cannot close, and a name
//! written outside any quote, as an identifier.
//!
//! These are `RoomKit`'s rules, decided as its Python implementation decides
//! them (`roomkit._text.fence`, `roomkit._text.identifier`). A provider
//! whose wire has no place of its own for such a text writes it in the
//! model's context with them.
//!
//! What Python decides of one character is read from tables generated from
//! Python (`tables`), so that both implementations read a text alike,
//! whatever Unicode version Rust's standard library carries.
//!
//! What holds, and what does not. `fence` decides as Python for a tag that
//! is a plain ASCII name, as every tag of `RoomKit` is: characters of the
//! ASCII range that show, none of them a space. The fixture of what Python
//! decides, and the runs that compared the two on generated tags, are on
//! such tags. For any other tag the text is still fenced, and a tag of that
//! name may be left where Python neutralises it. Three kinds are known:
//!
//! - a tag that ends with a combining mark: `fence("x\u{301}", "</x\u{301}>")`;
//! - a tag that starts with a space: `fence(" x", "</ x>")`;
//! - a tag with a letter the tables do not hold, whose other case or form
//!   Python reads as the same letter: `fence("\u{17f}", "</s>")`,
//!   `fence("\u{b5}", "</\u{3bc}>")`.
//!
//! `fence` asserts the tag it is given in a debug build.

mod fence;
mod identifier;
mod nfc;
mod tables;
mod tag_name;
mod unicode;

pub use self::fence::fence;
pub use self::identifier::identifier;

#[cfg(test)]
mod tests {
    /// The tables are derived from Unicode data: each carries the notice
    /// the crate ships as its license file.
    #[test]
    fn each_table_carries_the_notice_of_its_unicode_data() {
        let license = include_str!("../../LICENSE-UNICODE");
        let commented: String = license
            .lines()
            .map(|line| format!("//! {line}").trim_end().to_owned() + "\n")
            .collect();
        let tables = [
            include_str!("tables/classes.rs"),
            include_str!("tables/look_alikes.rs"),
            include_str!("tables/normalization.rs"),
        ];
        assert!(tables.iter().all(|table| table.contains(&commented)));
    }
}
