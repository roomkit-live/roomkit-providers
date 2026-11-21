//! What folding a stream's calls costs, and what bounds it: the bytes
//! given to the JSON parser while a call is folded grow with the call's
//! bytes, not with their square, whatever shape a server gives the call.

use serde_json::Value;

use super::{
    ARGUMENTS_LIMIT, CALLS_LIMIT, CallSlots, Fragment, FragmentId, NAME_LIMIT, RawCall, Unread,
};

/// A fragment on index 0: a name, when it brings one, and arguments.
type Piece<'a> = (Option<&'a str>, &'a str);

/// What folding gave: the bytes the JSON parser was given, the bytes of
/// arguments folded, and the calls.
struct Folded {
    parsed: usize,
    bytes: usize,
    calls: Vec<RawCall>,
}

/// Folds `pieces`, in order, all on index 0.
fn folded<'a>(pieces: impl IntoIterator<Item = Piece<'a>>) -> Result<Folded, Unread> {
    let mut slots = CallSlots::default();
    let mut bytes = 0;
    for (name, arguments) in pieces {
        bytes += arguments.len();
        slots.fold(&Fragment {
            index: 0,
            id: FragmentId::None,
            name,
            arguments,
            function: true,
        })?;
    }
    let parsed = slots.parsed;
    let calls = slots.finish()?;
    Ok(Folded {
        parsed,
        bytes,
        calls,
    })
}

/// The arguments of an ordinary call, `pairs` members long, as JSON.
fn document(pairs: usize) -> String {
    let members: Vec<String> = (0..pairs)
        .map(|at| format!("\"k{at}\": \"v{at}\""))
        .collect();
    format!("{{{}}}", members.join(", "))
}

/// `text` in pieces of four bytes.
fn in_fours(text: &str) -> Vec<&str> {
    let cuts = (0..text.len()).step_by(4);
    cuts.map(|at| text.get(at..(at + 4).min(text.len())).unwrap())
        .collect()
}

/// A piece that owns its arguments.
type Owned = (Option<&'static str>, String);

/// The shapes a call may be folded in, each `size` fragments or so: what
/// it is, and its pieces.
fn shapes(size: usize) -> Vec<(&'static str, Vec<Owned>)> {
    let ordinary = document(size / 4);
    let named = |name| move |piece: &str| (name, piece.to_owned());
    let lookup = Some("lookup");
    let start = |arguments: &str| vec![(lookup, arguments.to_owned())];
    let then = |mut pieces: Vec<Owned>, more: Vec<_>| {
        pieces.extend(more);
        pieces
    };
    let repeat = |name, piece: &str| vec![(name, piece.to_owned()); size];
    let edge = format!("{}.", "9".repeat(309));
    let other_names = |piece: &str| {
        let turns = [Some("one"), Some("two")].into_iter().cycle();
        turns
            .take(size)
            .map(|name| (name, piece.to_owned()))
            .collect::<Vec<_>>()
    };
    vec![
        (
            "an ordinary call",
            then(
                start(""),
                in_fours(&ordinary).into_iter().map(named(None)).collect(),
            ),
        ),
        (
            "its name on every fragment",
            in_fours(&ordinary).into_iter().map(named(lookup)).collect(),
        ),
        (
            "an object opened on every fragment",
            then(start("{"), repeat(None, "{\"a\": ")),
        ),
        (
            "an object opened on every fragment, named",
            then(start("{"), repeat(lookup, "{\"a\": ")),
        ),
        (
            "a document that does not read, then more",
            then(start("{\"a\" 1}"), repeat(None, "{x")),
        ),
        (
            "a document that does not read, then white space under other names",
            then(start("{\"a\" 1}"), other_names(" ")),
        ),
        (
            "a document that does not read, then more under other names",
            then(start("{\"a\" 1}"), other_names("{}")),
        ),
        (
            "a number at the edge of a float, digit by digit",
            then(start(&edge), repeat(lookup, "7")),
        ),
        (
            "a number at the edge of a float, under other names",
            then(start(&edge), other_names("7")),
        ),
        (
            "a number past a float, digit by digit",
            then(start(&"9".repeat(310)), other_names("7")),
        ),
        (
            "a number, digit by digit",
            then(start("1"), repeat(None, "7")),
        ),
        ("white space", then(start("{}"), repeat(lookup, " \n"))),
        (
            "white space before anything",
            then(start(" "), repeat(lookup, " \t")),
        ),
        (
            "a string that never closes",
            then(start("\""), repeat(lookup, "{\\\"")),
        ),
    ]
}

/// The bytes the parser is given to fold one call of `size` fragments in
/// `shape`: the call stays one, and holds every byte.
fn parsed_by(shape: &str, pieces: &[Owned]) -> (usize, usize) {
    let borrowed = pieces.iter().map(|(name, piece)| (*name, piece.as_str()));
    let folded = folded(borrowed).unwrap();
    assert_eq!(folded.calls.len(), 1, "{shape}");
    assert_eq!(folded.calls[0].arguments.len(), folded.bytes, "{shape}");
    (folded.parsed, pieces.len())
}

// The parser is given a number of bytes that grows with the fragments, not
// with their square: at most what the call weighs, and at most the short
// form of a number at the edge of a float for each fragment.
#[test]
fn folding_a_call_gives_the_parser_bytes_in_proportion_to_its_fragments() {
    const SHORT_FORM: usize = 330;
    for size in [1000, 4000] {
        for (shape, pieces) in shapes(size) {
            let (parsed, fragments) = parsed_by(shape, &pieces);
            let bytes: usize = pieces.iter().map(|(_, piece)| piece.len()).sum();
            assert!(
                parsed <= bytes + SHORT_FORM * fragments,
                "{shape}: {parsed} bytes parsed for {fragments} fragments of {bytes} bytes"
            );
        }
    }
    // Twice the fragments, twice the bytes at most.
    let smaller: Vec<usize> = shapes(1000)
        .iter()
        .map(|(shape, pieces)| parsed_by(shape, pieces).0)
        .collect();
    let larger: Vec<usize> = shapes(2000)
        .iter()
        .map(|(shape, pieces)| parsed_by(shape, pieces).0)
        .collect();
    for ((shape, _), (smaller, larger)) in shapes(1000).iter().zip(smaller.iter().zip(&larger)) {
        assert!(
            *larger <= 2 * smaller + SHORT_FORM,
            "{shape}: {smaller} then {larger}"
        );
    }
    // An ordinary call is never given to the parser while it is folded.
    assert_eq!(parsed_by("an ordinary call", &shapes(4000)[0].1).0, 0);
}

// The fold of the ordinary shapes is what it was: the cost changed, not
// the calls.
#[test]
fn the_calls_folded_are_what_asking_the_parser_each_time_gave() {
    let whole = |text: &str| {
        let held = text.trim();
        held.is_empty() || serde_json::from_str::<Value>(held).is_ok()
    };
    // Each piece: a name or none, and arguments. The calls expected are
    // read from the rule itself, asked of the parser at every fragment.
    let pieces: [&[Piece<'_>]; 6] = [
        &[
            (Some("now"), "{}"),
            (None, "{}"),
            (None, " "),
            (None, "{\"a\": 1}"),
        ],
        &[
            (Some("now"), "1"),
            (Some("now"), "2"),
            (Some("later"), "3"),
            (None, "{"),
        ],
        &[
            (Some("now"), "{\"a\" 1}"),
            (None, "{}"),
            (Some("later"), "{}"),
            (Some("later"), "{}"),
        ],
        &[
            (Some("now"), "tru"),
            (Some("now"), "e"),
            (Some("now"), " "),
            (Some("now"), "{}"),
        ],
        &[
            (Some("now"), " "),
            (Some("later"), "\"a"),
            (Some("now"), "b\""),
            (None, "{}"),
        ],
        &[
            (Some("now"), "1e308"),
            (Some("now"), "0"),
            (Some("now"), "0"),
            (None, "{}"),
        ],
    ];
    for pieces in pieces {
        let mut expected: Vec<(String, String)> = Vec::new();
        for (name, arguments) in pieces {
            let starts = expected.last().is_some_and(|(held_name, held)| {
                let complete = whole(held);
                let blank = held.trim().is_empty();
                match name {
                    None => !blank && complete && arguments.trim_start().starts_with('{'),
                    Some(name) if name != held_name => complete,
                    Some(_) => !arguments.trim().is_empty() && !blank && complete,
                }
            });
            if let Some((held_name, held)) = expected.last_mut().filter(|_| !starts) {
                held.push_str(arguments);
                if let Some(name) = name {
                    (*name).clone_into(held_name);
                }
            } else {
                let name = name.or(expected.last().map(|(name, _)| name.as_str()));
                let name = name.unwrap_or_default().to_owned();
                expected.push((name, (*arguments).to_owned()));
            }
        }
        let calls = folded(pieces.iter().copied()).unwrap().calls;
        let found: Vec<(String, String)> = calls
            .into_iter()
            .map(|call| (call.name, call.arguments))
            .collect();
        assert_eq!(found, expected, "{pieces:?}");
    }
}

// What a server controls of a call is bounded: how many calls an answer
// holds, a call's arguments, its name and its id.
#[test]
fn a_stream_s_calls_are_bounded() {
    let whole = |index: u64| Fragment {
        index,
        id: FragmentId::None,
        name: Some("now"),
        arguments: "{}",
        function: true,
    };
    let mut slots = CallSlots::default();
    for index in 0..CALLS_LIMIT {
        slots.fold(&whole(u64::try_from(index).unwrap())).unwrap();
    }
    let one_more = slots.fold(&whole(u64::MAX)).unwrap_err();
    assert_eq!(
        one_more,
        Unread::because("the answer holds more than 1024 calls")
    );

    let piece = "x".repeat(64 * 1024);
    let within = ARGUMENTS_LIMIT / piece.len();
    let fill = |pieces: usize| folded(std::iter::repeat_n((Some("now"), piece.as_str()), pieces));
    assert_eq!(fill(within).unwrap().bytes, ARGUMENTS_LIMIT);
    let too_long = Unread::because("a call's arguments are longer than 4194304 bytes");
    assert_eq!(fill(within + 1).err(), Some(too_long));

    let (longest, longer) = ("n".repeat(NAME_LIMIT), "n".repeat(NAME_LIMIT + 1));
    let named = |name: &str| folded([(Some(name), "{}")]).map(|folded| folded.calls.len());
    assert_eq!(named(&longest), Ok(1));
    let too_long = Unread::because("a call's name is longer than 1024 bytes");
    assert_eq!(named(&longer), Err(too_long));
    let with_id = |id: &str| {
        let mut fragment = whole(0);
        fragment.id = FragmentId::Text(id);
        CallSlots::default().fold(&fragment).map(|_| ())
    };
    assert_eq!(with_id(&longest), Ok(()));
    let too_long = Unread::because("a call's id is longer than 1024 bytes");
    assert_eq!(with_id(&longer), Err(too_long));
}
