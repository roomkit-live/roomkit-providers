//! The reading of held arguments against the rule it stands for: whatever
//! the text and however it is cut, it answers as asking the parser of the
//! whole text does.

use serde_json::Value;

use super::Held;

/// Whether Python's `str.strip()` strips `character`.
fn stripped(character: char) -> bool {
    character.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&character)
}

/// Whether Python's `str.strip()` leaves nothing of `text`.
fn blank(text: &str) -> bool {
    text.chars().all(stripped)
}

/// The rule, Python's `not arguments_cut(text)`: the text is nothing but
/// what `str.strip()` strips, or it reads as JSON, which takes a space, a
/// tab and a line break around a document and no other white space.
fn reads_whole(text: &str) -> bool {
    blank(text) || serde_json::from_str::<Value>(text).is_ok()
}

/// Reads `text` in the pieces `cuts` make of it, and requires the rule's
/// answer after each piece.
fn check(text: &str, cuts: &[usize]) {
    let mut held = Held::default();
    let (mut examined, mut from) = (0, 0);
    let ends = cuts.iter().copied().chain([text.len()]);
    for to in ends.filter(|to| text.is_char_boundary(*to)) {
        held.push(text.get(from..to).unwrap(), &mut examined);
        let read = text.get(..to).unwrap();
        assert_eq!(held.is_blank(), blank(read), "blank: {read:?}");
        let whole = held.is_whole(read, &mut examined);
        assert_eq!(whole, reads_whole(read), "whole: {read:?}");
        // Asked again, the answer is the same, and nothing more is looked
        // at to give it.
        let before = examined;
        assert_eq!(held.is_whole(read, &mut examined), whole, "again: {read:?}");
        assert_eq!(examined, before, "asked twice: {read:?}");
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

// White space is Python's: a text of nothing but what `str.strip()` strips
// is whole, the four separators of information included, and a document
// reads with a space, a tab or a line break around it and with no other
// white space, a no-break space or a line separator no more than a letter.
#[test]
fn the_white_space_around_a_document_is_python_s() {
    let documents = [
        "\u{a0}",
        "\u{b}\u{c}\u{85}",
        "\u{1c}",
        " \u{1d}\u{1e}\u{1f} ",
        "\u{200b}",
        " \t\r\n{}\n\r\t ",
        "\u{a0}{}",
        "{}\u{a0}",
        " \u{a0} {}",
        "{} \u{a0} ",
        "\u{b}{}",
        "{}\u{c}",
        "{}\u{85}",
        "{}\u{2028}",
        "\u{1c}{}",
        "{}\u{1f}",
        "{}\u{3000}",
        "\u{a0}12",
        "\u{a0}true",
        "\u{a0}\"text\"",
        "\"text\"\u{a0}",
        "true\u{a0}",
        "true\u{1c}",
        "null\u{2028}",
        "12\u{b}",
        "12\u{1c}",
        "{\u{a0}}",
        "[1,\u{a0}2]",
        "{\"a\u{a0}b\": \"\u{a0}\"}",
    ];
    for document in documents {
        check_every_way(document);
    }
    // What the rule itself says of them, so that the rule is Python's.
    for (text, whole) in [
        ("\u{a0}", true),
        ("\u{1c}", true),
        ("\u{a0}{}", false),
        ("{}\u{a0}", false),
        ("\u{1c}{}", false),
        ("{}\u{b}", false),
        (" {}\n", true),
        ("\u{feff}", false),
    ] {
        assert_eq!(reads_whole(text), whole, "{text:?}");
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
    let mut examined = 0;
    let mut text = String::new();
    let digits = "9".repeat(309) + "." + &"9".repeat(4000);
    for digit in digits.chars() {
        let piece = digit.to_string();
        held.push(&piece, &mut examined);
        text.push(digit);
        held.is_whole(&text, &mut examined);
    }
    // Each digit is read once as it comes, and the parser is asked once,
    // of the short form: the digits after the first 309 change nothing.
    assert!(
        examined <= text.len() + SHORT_FORM,
        "{examined} bytes looked at for {} bytes",
        text.len()
    );
    // Before the edge and past it, nothing is parsed at all.
    for digits in ["9".repeat(308), "9".repeat(4000)] {
        let (mut held, mut examined) = (Held::default(), 0);
        held.push(&digits, &mut examined);
        held.is_whole(&digits, &mut examined);
        assert_eq!(examined, digits.len());
    }
}

/// The most bytes of the short form a number at the edge is asked as.
const SHORT_FORM: usize = 330;

/// What is looked at to read `text` whole, then to answer `questions`
/// times whether it is whole.
fn examined_asking(text: &str, questions: usize) -> usize {
    let (mut held, mut examined) = (Held::default(), 0);
    held.push(text, &mut examined);
    let whole = held.is_whole(text, &mut examined);
    assert_eq!(whole, reads_whole(text), "{}", text.len());
    for _ in 0..questions {
        assert_eq!(held.is_whole(text, &mut examined), whole);
    }
    examined
}

// A question costs what came since the last one: asked again and again of
// the same text, whatever its size and its shape, it looks at nothing more.
#[test]
fn a_question_asked_again_of_the_same_text_looks_at_nothing_more() {
    let zeros = "0".repeat(250_000);
    let members: Vec<String> = (0..2_000).map(|at| format!("\"k{at}\": {at}")).collect();
    let document = format!("{{{}}}", members.join(", "));
    let texts = [
        // A number at the edge of a float, its first digit far in.
        format!("0.{zeros}2e250309"),
        format!("-0.{zeros}17976931348623157E+250309"),
        format!("0.{zeros}17976931348623159e250309"),
        // The same below the edge and past it.
        format!("0.{zeros}2e250308"),
        format!("0.{zeros}2e250310"),
        // A number as long as the largest float, and one with a fraction.
        largest(),
        format!("{}.{zeros}", largest()),
        format!("{}9.5", "9".repeat(308)),
        // A document that ended, one that reads and one that does not.
        document.clone(),
        format!("{document} \n"),
        format!("{} ", document.replacen(':', "", 1)),
        format!("{zeros} "),
        // A document still open, white space, and what is no document.
        document.trim_end_matches('}').to_owned(),
        " ".repeat(250_000),
        format!("{document}{document}"),
    ];
    for text in &texts {
        let once = examined_asking(text, 0);
        // Read once as it came, and given to the parser once at most.
        assert!(once <= 2 * text.len() + SHORT_FORM, "{}", text.len());
        assert_eq!(examined_asking(text, 1_000), once, "{}", text.len());
    }
}
