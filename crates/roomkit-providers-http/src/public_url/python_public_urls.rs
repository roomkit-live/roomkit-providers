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

/// The cases Python takes and this crate refuses, with its words. Python
/// takes each host for a name, which it does not resolve here.
const STRICTER: &[(&str, &str)] = &[
    // The client's parser refuses the host, or reads an address that is not
    // public.
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
        "hexadecimal_parts_of_no_digit_private_not_resolved",
        "URL host '0xa.0x.0x.0x' resolves to private address 10.0.0.0",
    ),
    (
        "punycode_that_decodes_to_nothing",
        "Invalid URL: invalid international domain name",
    ),
    // A name to the client's parser too, which spells a private address once
    // its trailing dots are dropped.
    (
        "hexadecimal_part_of_no_digit_private_with_dots_not_resolved",
        "URL host '10.0x' resolves to private address 10.0.0.0",
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

/// The cases Python refuses and this crate takes. Each is a host the
/// client's parser reads as a public address, which the client dials: Python
/// takes it for a name, asks a resolver, and gets no answer.
const READ_AS_AN_ADDRESS: &[&str] = &["hexadecimal_parts_of_no_digit"];

/// The cases both take, where this crate asks a resolver and Python asks
/// none, with the name asked. Each is a public address followed by two dots
/// or more: Python drops the dots and reads the address; the client's parser
/// reads a name, which the client resolves, so the name is resolved here,
/// without its dots, and its answers checked. The fixture's table answers
/// what a resolver answers: the address the text spells.
const ASKED_OF_A_RESOLVER: &[(&str, &str)] = &[
    ("ipv4_short_public_with_dots", "1.2.3"),
    ("ipv4_public_with_dots", "93.184.216.34"),
];

impl Fixture {
    fn case(&self, name: &str) -> &Case {
        let found = self.cases.iter().find(|case| case.name == name);
        found.unwrap_or_else(|| panic!("the fixture has no case {name}"))
    }
}

/// What the validator answers for `case`, and the names it asked for, each
/// resolved from the fixture's table. A name the table does not hold has no
/// address: Python asked for every name the table holds and no other, so the
/// lookup shows as a difference.
fn decided(case: &Case, answers: &Answers) -> (Option<String>, Vec<(String, u16)>) {
    let schemes: Vec<&str> = case.allowed_schemes.iter().map(String::as_str).collect();
    let options = ValidatePublicUrlOptions {
        allowed_schemes: &schemes,
        resolve_dns: case.resolve_dns,
    };
    let mut lookups = Vec::new();
    let result = validate(&case.url, &options, |host, port| {
        lookups.push((host.to_owned(), port));
        answers.get(host).cloned().flatten().unwrap_or_default()
    });
    if let Ok(taken) = result {
        assert_eq!(taken, case.url, "{}: a URL taken is given back", case.name);
    }
    (result.err(), lookups)
}

/// The fixture, and the names of the cases the lists above set apart.
fn fixture() -> (Fixture, HashSet<&'static str>) {
    let listed = [STRICTER, OTHER_WORDS, ASKED_OF_A_RESOLVER];
    let named = listed
        .iter()
        .flat_map(|list| list.iter().map(|(name, _)| *name));
    let apart = named.chain(READ_AS_AN_ADDRESS.iter().copied());
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
    assert!(fixture.cases.len() >= 398, "the fixture lost cases");
}

/// The characters this crate refuses in an authority are the ones Python's
/// Unicode data turns into a delimiter.
#[test]
fn the_characters_that_normalize_to_a_delimiter_are_python_s() {
    let (fixture, _) = fixture();
    assert_eq!(DELIMITER_LOOKALIKES.to_vec(), fixture.delimiter_lookalikes);
}

/// The list of those characters is computed from Unicode data: the crate
/// ships the data's notice beside its own license, in what it publishes,
/// and declares both.
#[test]
fn the_crate_ships_and_declares_the_license_of_its_unicode_list() {
    let license = include_str!("../../LICENSE-UNICODE");
    assert!(license.starts_with("UNICODE LICENSE V3\n"));
    assert_eq!(env!("CARGO_PKG_LICENSE"), "MIT AND Unicode-3.0");
    let manifest = include_str!("../../Cargo.toml");
    for shipped in ["\"/LICENSE\"", "\"/LICENSE-UNICODE\""] {
        assert!(manifest.contains(shipped), "{shipped}");
    }
}

/// The cases set apart that this crate refuses: each in its own words, and
/// no name asked for it.
#[test]
fn the_cases_set_apart_are_refused_in_this_crate_s_words() {
    let (fixture, _) = fixture();
    let case = |name: &str| fixture.case(name);
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

/// The cases set apart that this crate takes: each without a lookup, where
/// Python refuses a name that has no answer.
#[test]
fn a_host_the_parser_reads_as_a_public_address_is_taken_without_a_lookup() {
    let (fixture, _) = fixture();
    for name in READ_AS_AN_ADDRESS {
        let case = fixture.case(name);
        let python = case.error.as_deref();
        assert!(
            python.is_some_and(|said| said.ends_with("did not resolve to any address")),
            "{name}: {python:?}"
        );
        assert_eq!(
            decided(case, &fixture.answers),
            (None, Vec::new()),
            "{name}"
        );
    }
}

/// The cases set apart that both take: Python without a lookup, this crate
/// once the name the client resolves has answered.
#[test]
fn a_public_address_followed_by_dots_is_resolved_as_the_name_the_client_reads() {
    let (fixture, _) = fixture();
    for (name, asked) in ASKED_OF_A_RESOLVER {
        let case = fixture.case(name);
        assert!(case.accepted && case.lookups.is_empty(), "{name}");
        assert_eq!(
            decided(case, &fixture.answers),
            (None, vec![((*asked).to_owned(), 80)]),
            "{name}"
        );
    }
}
