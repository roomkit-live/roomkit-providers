//! What Python sent and was answered: where each request went, what each
//! exchange holds, the waits, the reads of a stream, the ids Python minted,
//! what the SDK decided and the kinds of case.

use serde_json::{Value, json};

use super::super::Says::{Holding, Only};
use super::super::Wrong;
use super::super::sound::{KEY, MINTED};
use super::{answer, case, sent, wait, without};

/// `each_request_was_sent_where_its_case_expects` and
/// `the_timeouts_are_the_ones_python_sent`.
pub(super) const SENT: &[Wrong] = &[
    (
        "a request sent elsewhere",
        |f| sent(f, "answered", 0)["url"] = "https://llm.example/v1/other".into(),
        Holding("failed: answered\n  left: \"https://llm.example/v1/other\""),
    ),
    (
        "a request after a redirect sent elsewhere than its location",
        |f| answer(f, "redirected", 0)["headers"]["location"] = "/another".into(),
        Holding(
            "failed: redirected\n  left: \"https://llm.example/elsewhere\"\n right: \"https://llm.example/another\"",
        ),
    ),
    (
        "a location that is no URL",
        |f| answer(f, "redirected", 0)["headers"]["location"] = "https://[".into(),
        Holding("called `Result::unwrap()` on an `Err` value: InvalidIpv6Address"),
    ),
    (
        "a redirect from what is no URL",
        |f| {
            case(f, "redirected")["config"]["base_url"] = "nowhere".into();
            sent(f, "redirected", 0)["url"] = "nowhere/answers".into();
        },
        Holding("called `Result::unwrap()` on an `Err` value: RelativeUrlWithoutBase"),
    ),
    (
        "a redirect followed that the fixture does not question",
        |f| {
            let moved = answer(f, "tried_again", 0);
            moved["status"] = 302.into();
            moved["headers"]["location"] = "/v1/answers".into();
            without(&mut case(f, "tried_again")["exchanges"][0], "wait");
        },
        Only("tried_again"),
    ),
    (
        "a request sent with another timeout",
        |f| sent(f, "answered", 0)["timeouts"]["read"] = 30.0.into(),
        Holding("failed: answered\n  left: {\"connect\": 5.0, \"pool\": 60.0, \"read\": 30.0"),
    ),
];

/// `each_exchange_holds_a_request_and_one_answer`: the request, and the
/// one body of its answer.
pub(super) const EXCHANGES: &[Wrong] = &[
    (
        "a POST with no body",
        |f| sent(f, "answered", 0)["body"] = Value::Null,
        Holding("failed: answered\n  left: true\n right: false"),
    ),
    (
        "a listing that is posted",
        |f| {
            sent(f, "listed", 0)["method"] = "POST".into();
            sent(f, "listed", 0)["body"] = json!({});
        },
        Holding("failed: listed\n  left: \"POST\"\n right: \"GET\""),
    ),
    (
        "a listing that says what its body is",
        |f| sent(f, "listed", 0)["headers"]["content-type"] = "application/json".into(),
        Holding("failed: listed\n  left: Some(Some(\"application/json\"))\n right: Some(None)"),
    ),
    (
        "a listing that does not record its content type",
        |f| without(&mut sent(f, "listed", 0)["headers"], "content-type"),
        Holding("failed: listed\n  left: None\n right: Some(None)"),
    ),
    (
        "a method no provider sends",
        |f| {
            sent(f, "answered", 0)["method"] = "PUT".into();
            sent(f, "answered", 0)["body"] = Value::Null;
        },
        Only("answered"),
    ),
    (
        "a key in a recorded header",
        |f| sent(f, "answered", 0)["headers"]["authorization"] = format!("Bearer {KEY}").into(),
        Only("answered: a key in a header"),
    ),
    (
        "two bodies",
        |f| answer(f, "answered", 0)["text"] = "Yes.".into(),
        Holding("failed: answered\n  left: 2\n right: 1"),
    ),
    (
        "no body",
        |f| without(answer(f, "answered", 0), "json"),
        Holding("failed: answered\n  left: 0\n right: 1"),
    ),
    (
        "a failure in place of an answer that has a status",
        |f| answer(f, "silent", 0)["status"] = 500.into(),
        Holding("failed: silent\n  left: true\n right: false"),
    ),
    (
        "a failure in place of an answer that has headers",
        |f| answer(f, "said_by_the_sdk", 0)["headers"] = json!({}),
        Holding("failed: said_by_the_sdk\n  left: true\n right: false"),
    ),
    (
        "a whole body that says how its reads end",
        |f| answer(f, "answered", 0)["then"] = "the stream ends".into(),
        Holding("failed: answered\n  left: true\n right: false"),
    ),
    (
        "reads that end in no known way",
        |f| answer(f, "streamed", 0)["then"] = "the stream bursts".into(),
        Only("streamed: the stream bursts"),
    ),
];

/// `each_exchange_holds_a_request_and_one_answer`: the waits.
pub(super) const WAITS: &[Wrong] = &[
    (
        "a wait of no time",
        |f| case(f, "tried_again")["exchanges"][0]["wait"] = wait(0.0),
        Only("tried_again"),
    ),
    (
        "a wait longer than the SDK takes",
        |f| case(f, "tried_again")["exchanges"][0]["wait"] = wait(60.5),
        Only("tried_again"),
    ),
    (
        "a wait that does not say where it comes from",
        |f| case(f, "tried_again")["exchanges"][0]["wait"]["from"] = "a backoff".into(),
        Only("tried_again"),
    ),
    (
        "a wait after the last exchange",
        |f| case(f, "tried_again")["exchanges"][1]["wait"] = wait(1.0),
        Holding("failed: tried_again\n  left: true\n right: false"),
    ),
    (
        "no wait before the next try",
        |f| without(&mut case(f, "tried_again")["exchanges"][0], "wait"),
        Holding("failed: tried_again\n  left: false\n right: true"),
    ),
    (
        "a wait before a redirect is followed",
        |f| case(f, "redirected")["exchanges"][0]["wait"] = wait(0.5),
        Holding("failed: redirected\n  left: true\n right: false"),
    ),
];

/// `each_chunk_reads_as_the_bytes_of_one_read`.
pub(super) const CHUNKS: &[Wrong] = &[
    (
        "bytes that are no hexadecimal",
        |f| answer(f, "cut_inside_a_character", 0)["chunks"][0]["hex"] = "c3zz".into(),
        Only("cut_inside_a_character: a chunk"),
    ),
    (
        "bytes cut inside a digit",
        |f| answer(f, "cut_inside_a_character", 0)["chunks"][0]["hex"] = "c3a".into(),
        Only("cut_inside_a_character: a chunk"),
    ),
    (
        "bytes that read as a text, written in hexadecimal",
        |f| answer(f, "streamed", 0)["chunks"] = json!([{"hex": "0a0a"}]),
        Only("streamed"),
    ),
    (
        "reads cut inside a character in a case of another kind",
        |f| {
            let cut = answer(f, "cut_inside_a_character", 0)["chunks"].clone();
            answer(f, "streamed_as_text", 0)["chunks"] = cut;
        },
        Holding("failed: streamed_as_text\n  left: true\n right: false"),
    ),
    (
        "no read cut inside a character in a case of that kind",
        |f| answer(f, "cut_inside_a_character", 0)["chunks"] = json!(["data: é\n\n"]),
        Holding("failed: cut_inside_a_character\n  left: false\n right: true"),
    ),
    (
        "reads that together are no text",
        |f| {
            answer(f, "cut_inside_a_character", 0)["chunks"] =
                json!([{"hex": "c3"}, {"hex": "c3"}]);
        },
        Only("cut_inside_a_character"),
    ),
];

/// `each_minted_call_id_has_python_s_shape`, `a_case_the_sdk_decides_says_so`
/// and `covers_every_kind_of_case`.
pub(super) const THE_REST: &[Wrong] = &[
    (
        "a minted id its case does not give",
        |f| case(f, "minted")["minted_call_ids"] = json!(["call_lookup_ba9876543210"]),
        Only("minted"),
    ),
    (
        "a minted id that does not start as Python's",
        |f| mint(f, "tool_lookup_0123456789ab"),
        Only("minted"),
    ),
    (
        "a minted id that does not end as Python's",
        |f| mint(f, "call_lookup_0123456789"),
        Only("minted"),
    ),
    (
        "a case of minted ids that holds none",
        |f| case(f, "minted")["minted_call_ids"] = json!([]),
        Only("minted"),
    ),
    (
        "a request tried again that does not say the SDK decided it",
        |f| without(case(f, "tried_again"), "sdk"),
        Only("tried_again"),
    ),
    (
        "the SDK's words in a case that does not say the SDK decided it",
        |f| without(case(f, "said_by_the_sdk"), "sdk"),
        Only("said_by_the_sdk"),
    ),
    (
        "a case written twice",
        |f| {
            let again = case(f, "answered").clone();
            f["cases"].as_array_mut().unwrap().push(again);
        },
        Only("answered twice"),
    ),
    (
        "a kind the test does not list",
        |f| case(f, "answered")["kinds"] = json!(["answer", "unheard"]),
        Only("answered: unheard"),
    ),
    (
        "a kind no case shows",
        |f| case(f, "minted")["kinds"] = json!(["answer"]),
        Holding("left: {\"answer\", \"failure\", \"listing\", \"redirect\", \"retry\", \"stream\""),
    ),
];

/// Writes `id` as the one id Python minted in the case `minted`, in its
/// result too.
fn mint(fixture: &mut Value, id: &str) {
    let minted = case(fixture, "minted");
    assert_eq!(
        minted["expected"]["response"]["tool_calls"][0]["id"],
        MINTED
    );
    minted["expected"]["response"]["tool_calls"][0]["id"] = id.into();
    minted["minted_call_ids"] = json!([id]);
}
