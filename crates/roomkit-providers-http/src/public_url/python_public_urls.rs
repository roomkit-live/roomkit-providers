//! The validator against what `RoomKit`'s Python decides of a table of URLs:
//! `tests/fixtures/python_public_urls.json`, written by
//! `scripts/generate_python_public_urls.py` (`make python-public-urls`).
//!
//! Each case holds a URL, the options Python's `validate_public_url` was
//! given, and what it answered: the URL taken, or the words of its
//! `ValueError`, with the names it asked a resolver for. Python resolved
//! them from a table the fixture carries, and so does this test: no case
//! asks a name of the network.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use serde::Deserialize;

use super::authority::DELIMITER_LOOKALIKES;
use super::{ValidatePublicUrlOptions, validate};

const FIXTURE: &str = include_str!("../../tests/fixtures/python_public_urls.json");

/// The addresses of each name, `None` for a name with no answer.
type Answers = HashMap<String, Option<Vec<IpAddr>>>;

#[derive(Deserialize)]
struct Fixture {
    delimiter_lookalikes: Vec<char>,
    answers: Answers,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    url: String,
    allowed_schemes: Vec<String>,
    resolve_dns: bool,
    accepted: bool,
    error: Option<String>,
    lookups: Vec<(String, u16)>,
}

/// What the client's parser says of a host whose last label is a number
/// and that is no IPv4 address.
const NO_IPV4: &str = "Invalid URL: invalid IPv4 address";

/// What this crate says of a bracketed host that is no IPv6 address.
const NO_IPV6: &str = "Invalid IPv6 host or port in URL authority";

/// The cases Python takes and this crate refuses, with its words. Each is a
/// host the client's parser reads otherwise than Python reads it: Python
/// takes it for a name, which it does not resolve here.
const STRICTER: &[(&str, &str)] = &[
    ("number_past_the_largest_not_resolved", NO_IPV4),
    ("part_past_255_not_resolved", NO_IPV4),
    ("five_parts_not_resolved", NO_IPV4),
    ("octal_with_an_eight_not_resolved", NO_IPV4),
    ("name_ending_in_a_number_not_resolved", NO_IPV4),
    ("name_ending_in_a_hexadecimal_number_not_resolved", NO_IPV4),
    (
        "hexadecimal_of_no_digit_not_resolved",
        "URL host '0x' resolves to private address 0.0.0.0",
    ),
    (
        "punycode_that_decodes_to_nothing",
        "Invalid URL: invalid international domain name",
    ),
];

/// The cases both refuse, in other words, with this crate's.
const OTHER_WORDS: &[(&str, &str)] = &[
    // Python takes each for a name, asks a resolver, and gets no answer.
    ("number_past_the_largest", NO_IPV4),
    ("part_past_255", NO_IPV4),
    ("five_parts", NO_IPV4),
    ("octal_with_an_eight", NO_IPV4),
    ("name_ending_in_a_number", NO_IPV4),
    ("name_ending_in_a_hexadecimal_number", NO_IPV4),
    (
        "hexadecimal_of_no_digit",
        "URL host '0x' resolves to private address 0.0.0.0",
    ),
    // Python's words are its `ipaddress` module's, one wording per fault.
    ("bracket_two_parts", NO_IPV6),
    ("bracket_ipv4", NO_IPV6),
    ("bracket_two_runs", NO_IPV6),
    ("ipv6_ipv4_with_a_zero", NO_IPV6),
    ("ipv6_ipv4_of_three_parts", NO_IPV6),
    ("ipv6_nine_parts", NO_IPV6),
    ("ipv6_part_of_five_digits", NO_IPV6),
    // Python's words come from an address check its `urlsplit` runs on the
    // name once the user information holds a pair of brackets.
    ("bracket_pair_in_the_user", "Invalid IPv6 URL"),
    // Python escapes a character past ASCII that is not printable; this
    // crate writes it as it is.
    (
        "port_no_break_space",
        "Port could not be cast to integer value as '\u{a0}'",
    ),
];

/// What the validator answers for `case`, and the names it asked for, each
/// resolved from the fixture's table.
fn decided(case: &Case, answers: &Answers) -> (Option<String>, Vec<(String, u16)>) {
    let schemes: Vec<&str> = case.allowed_schemes.iter().map(String::as_str).collect();
    let options = ValidatePublicUrlOptions {
        allowed_schemes: &schemes,
        resolve_dns: case.resolve_dns,
    };
    let mut lookups = Vec::new();
    let result = validate(&case.url, &options, |host, port| {
        lookups.push((host.to_owned(), port));
        let Some(found) = answers.get(host) else {
            panic!(
                "{}: the fixture has no answer for {host:?}, which would need the network",
                case.name
            );
        };
        found.clone().unwrap_or_default()
    });
    if let Ok(taken) = result {
        assert_eq!(taken, case.url, "{}: a URL taken is given back", case.name);
    }
    (result.err(), lookups)
}

/// The fixture, and the names of the cases the lists above set apart.
fn fixture() -> (Fixture, HashSet<&'static str>) {
    let apart = STRICTER.iter().chain(OTHER_WORDS).map(|(name, _)| *name);
    (serde_json::from_str(FIXTURE).unwrap(), apart.collect())
}

/// Every case but the ones set apart: the same URLs taken, the same words
/// for the ones refused, the same names asked at the same ports.
#[test]
fn the_validator_decides_as_python_does() {
    let (fixture, apart) = fixture();
    let mut differing = Vec::new();
    for case in &fixture.cases {
        if apart.contains(case.name.as_str()) {
            continue;
        }
        assert_eq!(case.accepted, case.error.is_none(), "{}", case.name);
        let (error, lookups) = decided(case, &fixture.answers);
        if (&error, &lookups) != (&case.error, &case.lookups) {
            differing.push(format!(
                "{} {:?}: {error:?} {lookups:?}, where Python says {:?} {:?}",
                case.name, case.url, case.error, case.lookups
            ));
        }
    }
    assert_eq!(differing, Vec::<String>::new());
    assert!(fixture.cases.len() >= 370, "the fixture lost cases");
}

/// The characters this crate refuses in an authority are the ones Python's
/// Unicode data turns into a delimiter.
#[test]
fn the_characters_that_normalize_to_a_delimiter_are_python_s() {
    let (fixture, _) = fixture();
    assert_eq!(DELIMITER_LOOKALIKES.to_vec(), fixture.delimiter_lookalikes);
}

/// The cases set apart: this crate refuses each, in its own words, and asks
/// no name for it.
#[test]
fn the_cases_set_apart_are_refused_in_this_crate_s_words() {
    let (fixture, _) = fixture();
    let case = |name: &str| {
        let found = fixture.cases.iter().find(|case| case.name == name);
        found.unwrap_or_else(|| panic!("the fixture has no case {name}"))
    };
    for (name, words) in STRICTER {
        assert!(case(name).accepted, "{name}: Python takes it");
        let (error, lookups) = decided(case(name), &fixture.answers);
        assert_eq!(
            (error.as_deref(), lookups),
            (Some(*words), Vec::new()),
            "{name}"
        );
    }
    for (name, words) in OTHER_WORDS {
        let python = case(name).error.as_deref();
        assert!(
            python.is_some_and(|said| said != *words),
            "{name}: {python:?}"
        );
        let (error, lookups) = decided(case(name), &fixture.answers);
        assert_eq!(
            (error.as_deref(), lookups),
            (Some(*words), Vec::new()),
            "{name}"
        );
    }
}
