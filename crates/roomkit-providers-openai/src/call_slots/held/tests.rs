//! The reading of held arguments against the rule it stands for: whatever
//! the text and however it is cut, it answers as asking the parser of the
//! whole text does.

use serde_json::Value;

use super::Held;

/// The rule: the text, without the white space around it, is empty or
/// reads as JSON.
fn reads_whole(text: &str) -> bool {
    let held = text.trim();
    held.is_empty() || serde_json::from_str::<Value>(held).is_ok()
}

/// Reads `text` in the pieces `cuts` make of it, and requires the rule's
/// answer after each piece.
fn check(text: &str, cuts: &[usize]) {
    let mut held = Held::default();
    let (mut parsed, mut from) = (0, 0);
    let ends = cuts.iter().copied().chain([text.len()]);
    for to in ends.filter(|to| text.is_char_boundary(*to)) {
        held.push(text.get(from..to).unwrap());
        let read = text.get(..to).unwrap();
        assert_eq!(held.is_blank(), read.trim().is_empty(), "blank: {read:?}");
        let whole = held.is_whole(read, &mut parsed);
        assert_eq!(whole, reads_whole(read), "whole: {read:?}");
        // Asked again, the answer is the same.
        assert_eq!(held.is_whole(read, &mut parsed), whole, "again: {read:?}");
        from = to;
    }
}

/// `text` read one character at a time, then in every two pieces.
fn check_every_way(text: &str) {
    let every: Vec<usize> = (1..text.len()).collect();
    check(text, &every);
    for cut in 1..text.len() {
        check(text, &[cut]);
    }
    check(text, &[]);
}

/// The digits of the largest float, which a number at the edge of what
/// the parser reads starts with.
fn largest() -> String {
    format!("{:.0}", f64::MAX)
}

#[test]
fn a_document_reads_whole_as_the_parser_says_however_it_is_cut() {
    let documents = [
        "",
        " \t\r\n",
        "\u{a0}\u{2028} ",
        "{}",
        " {} ",
        "\u{a0}{}\u{2029}",
        "{}\u{a0}x",
        r#"{"a": {"b": [1, 2, {"c": "}]\"\\"}]}, "d": null}"#,
        r#"{"a": "b"} "#,
        r#"{"a": "b"} {"c": 1}"#,
        r#"{"a": "b"}x"#,
        r#"{"a" 1}"#,
        r#"{"a" 1} "#,
        r#"{"a" 1} more"#,
        "{]",
        "[}",
        "[[[[]]]]",
        "[1, 2",
        "[1, 2]]",
        "}",
        "]{}",
        "\u{feff}{}",
        r#""text""#,
        r#""te\"xt\\""#,
        r#""te\"xt\\" x"#,
        "\"open",
        "\"line\nbreak\"",
        "true",
        "tru",
        "true ",
        "truex",
        "true x",
        "tr ue",
        "false",
        "null",
        "nul ",
        "nulll",
        "NaN",
        "Infinity",
        "x",
        "+1",
        "-",
        "- 1",
        "-0",
        "-0.0e-0",
        "0",
        "00",
        "01",
        "-01",
        "0.",
        "0.5",
        "0.5.5",
        ".5",
        "1.",
        "1.e5",
        "1e",
        "1e+",
        "1e+5",
        "1E-5",
        "1e5e5",
        "1e5.5",
        "12 ",
        "12 3",
        "12\u{a0}",
        "12\u{a0}3",
        "12,",
        "12}",
        "12{",
        "1e308",
        "1e309",
        "9e308",
        "1.7976931348623157e308",
        "1.7976931348623158e308",
        "1.7976931348623159e308",
        "-1.7976931348623159e308",
        "0.17976931348623157e309",
        "0.17976931348623159e309",
        "0.00017976931348623157e312",
        "0.00017976931348623159e312",
        "17976931348623157e292",
        "17976931348623159e292",
        "179769313486231570000e288",
        "0e999999999999999999999",
        "0.000e999999999999999999999",
        "1e999999999999999999999",
        "1e-999999999999999999999",
        "0.0000000001e318",
        "0.0000000001e319",
        "0.0000000002e319",
        "1e0000000000000000000000308",
        "1e0000000000000000000000309",
        "18446744073709551615",
        "18446744073709551616",
        "-9223372036854775809",
    ];
    for document in documents {
        check_every_way(document);
    }
}

// Numbers at the edge of what a float holds, where the parser's answer
// turns: as long as the largest float, a digit more or less, with digits
// after the point, and around the number halfway to the next power of two.
#[test]
fn a_number_at_the_edge_of_a_float_reads_whole_as_the_parser_says() {
    let largest = largest();
    assert_eq!(largest.len(), 309);
    let (start, rest) = largest.split_at(17);
    assert_eq!(start, "17976931348623157");
    let zeros = "0".repeat(rest.len());
    let mut numbers = vec![
        largest.clone(),
        format!("{largest}0"),
        format!("{largest}.5"),
        format!("-{largest}.999"),
        largest.get(..308).unwrap().to_owned(),
        format!("9{}", "9".repeat(307)),
        format!("9{}", "9".repeat(308)),
        format!("1{}", "0".repeat(308)),
        format!("1{}", "0".repeat(309)),
        format!("{}.{}", "9".repeat(309), "9".repeat(40)),
        format!("0.{}{}e309", "0".repeat(3), "9".repeat(20)),
    ];
    // Around the first digits, and around the half way past the largest:
    // 1.7976931348623158079...e308.
    for digits in [
        "17976931348623157",
        "17976931348623158",
        "179769313486231580",
        "1797693134862315807",
        "17976931348623158079",
        "1797693134862315808",
        "17976931348623158080",
        "17976931348623159",
        "2",
    ] {
        let padded = format!("{digits}{}", "0".repeat(309 - digits.len()));
        numbers.push(format!("{padded}.000001"));
        numbers.push(format!("{}1", padded.get(..308).unwrap()));
        numbers.push(padded);
    }
    numbers.push(format!("{start}{zeros}"));
    for number in &numbers {
        check(number, &[]);
        check(number, &[1]);
        check(number, &[number.len() - 1]);
        // Digit by digit, from the edge on.
        let from = number.len().saturating_sub(12);
        let late: Vec<usize> = (from..number.len()).collect();
        check(number, &late);
    }
    // One of them, read one character at a time from its start.
    let every: Vec<usize> = (1..largest.len()).collect();
    check(&format!("{largest}.25"), &every);
}

// A number as long as a hostile server likes costs the parser nothing
// that grows with it: only a number at the edge is asked, written short.
#[test]
fn a_number_s_reading_gives_the_parser_no_more_than_its_edge() {
    let mut held = Held::default();
    let mut parsed = 0;
    let mut text = String::new();
    let mut asked = 0;
    let digits = "9".repeat(309) + "." + &"9".repeat(4000);
    for digit in digits.chars() {
        let piece = digit.to_string();
        held.push(&piece);
        text.push(digit);
        held.is_whole(&text, &mut parsed);
        asked += 1;
    }
    assert!(
        parsed <= 330 * asked,
        "{parsed} bytes for {asked} questions"
    );
    // Before the edge and past it, nothing is parsed at all.
    for digits in ["9".repeat(308), "9".repeat(4000)] {
        let (mut held, mut parsed) = (Held::default(), 0);
        held.push(&digits);
        held.is_whole(&digits, &mut parsed);
        assert_eq!(parsed, 0);
    }
}
