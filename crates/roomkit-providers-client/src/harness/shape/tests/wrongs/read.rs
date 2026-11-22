//! What a fixture says of itself and of its requests: every member read
//! strictly, where the fixture comes from, and each request as the contract
//! reads it.

use roomkit_ai::AiContext;
use roomkit_providers_python_forms::Form;
use serde_json::json;

use super::super::Says::{Holding, Only};
use super::super::Wrong;
use super::{answer, backwards, case, error, sent, without};

/// Every member of a fixture is read, strictly: a member that is missing,
/// and one no part of a case holds.
pub(super) const STRICTLY_READ: &[Wrong] = &[
    (
        "no clock",
        |f| without(f, "clock"),
        Holding("missing field `clock`"),
    ),
    (
        "an unknown member of an exchange",
        |f| case(f, "answered")["exchanges"][0]["more"] = 1.into(),
        Holding("unknown field `more`, expected one of `request`, `response`, `wait`"),
    ),
    (
        "an unknown member of a request sent",
        |f| sent(f, "answered", 0)["more"] = 1.into(),
        Holding("unknown field `more`, expected one of `method`"),
    ),
    (
        "an unknown member of an answer",
        |f| answer(f, "answered", 0)["more"] = 1.into(),
        Holding("unknown field `more`, expected one of `status`"),
    ),
    (
        "an unknown member of a wait",
        |f| case(f, "tried_again")["exchanges"][0]["wait"]["more"] = 1.into(),
        Holding("unknown field `more`, expected `seconds` or `from`"),
    ),
    (
        "an unknown member of a result",
        |f| case(f, "answered")["expected"]["more"] = 1.into(),
        Holding("unknown field `more`, expected one of `response`"),
    ),
    (
        "an unknown member of an error",
        |f| error(f, "refused")["more"] = 1.into(),
        Holding("unknown field `more`, expected one of `class`"),
    ),
    (
        "an unknown member of a comparison",
        |f| case(f, "refused")["compare"]["more"] = 1.into(),
        Holding("unknown field `more`, expected `exactly` or `message`"),
    ),
    (
        "an unknown member of a catalogue entry",
        |f| case(f, "answered")["catalogue"]["more"] = 1.into(),
        Holding("unknown field `more`, expected one of `model`"),
    ),
    (
        "a read that is neither a text nor bytes",
        |f| answer(f, "streamed", 0)["chunks"][0] = json!({"hex": "0a", "more": 1}),
        Holding("did not match any variant of untagged enum Chunk"),
    ),
    (
        "a member of a case its provider does not name",
        |f| case(f, "answered")["more"] = 1.into(),
        Only("answered: an unknown member more"),
    ),
];

/// `says_where_it_comes_from`.
pub(super) const WHERE_IT_COMES_FROM: &[Wrong] = &[
    (
        "no version of the SDK",
        |f| without(f, "vendor_version"),
        Only("no vendor_version"),
    ),
    (
        "a version of RoomKit that is no number",
        |f| f["roomkit_version"] = "main".into(),
        Only("main"),
    ),
    (
        "a version of the SDK that is no number",
        |f| f["vendor_version"] = "nightly".into(),
        Only("nightly"),
    ),
    (
        "a version of pydantic that is no number",
        |f| f["pydantic_version"] = "latest".into(),
        Only("latest"),
    ),
    (
        "a version of httpx2 that is no number",
        |f| f["httpx2_version"] = "newest".into(),
        Only("newest"),
    ),
    (
        "a version of Python that is no number",
        |f| f["python_version"] = "cpython".into(),
        Only("cpython"),
    ),
    (
        "a commit cut short",
        |f| f["roomkit_commit"] = "0123abc".into(),
        Only("0123abc"),
    ),
    (
        "a commit that is no hexadecimal",
        |f| f["roomkit_commit"] = "z".repeat(40).into(),
        Holding("zzzzzzzz"),
    ),
    (
        "files that were not clean",
        |f| f["roomkit_dirty"] = true.into(),
        Holding("left: Some(true)\n right: Some(false)"),
    ),
    (
        "a commit that does not say whether its files were clean",
        |f| without(f, "roomkit_dirty"),
        Holding("left: None\n right: Some(false)"),
    ),
    (
        "clean files of no commit",
        |f| without(f, "roomkit_commit"),
        Holding("left: Some(false)\n right: None"),
    ),
    (
        "another clock",
        |f| f["clock"] = "2026-01-02T00:00:00Z".into(),
        Holding("left: \"2026-01-02T00:00:00Z\""),
    ),
    (
        "no note on the questioned cases",
        |f| without(&mut f["notes"], "questioned"),
        Only("no note on questioned"),
    ),
    (
        "a note on the environment that names no variable",
        |f| f["notes"]["environment"] = "Nothing is removed.".into(),
        Holding("self.notes[\"environment\"].contains(shape.environment)"),
    ),
    (
        "a note on the answers that says nothing of bytes",
        |f| f["notes"]["exchanges.response"] = "Every read is a text.".into(),
        Holding("self.notes[\"exchanges.response\"].contains(\"hex\")"),
    ),
];

/// `each_request_reads_as_the_contract_s`.
pub(super) const REQUESTS: &[Wrong] = &[
    (
        "a request whose members come in another order",
        |f| backwards(&mut case(f, "answered")["request"]),
        Holding("failed: answered\n  left: Some("),
    ),
    (
        "a masked key with no plain key beside it",
        |f| without(case(f, "keyed"), "request_api_key"),
        Holding("failed: keyed\n  left: false\n right: true"),
    ),
    (
        "a plain key the request does not carry",
        |f| case(f, "answered")["request_api_key"] = "placeholder-lent-key".into(),
        Holding("failed: answered\n  left: true\n right: false"),
    ),
    (
        "a key no request may carry",
        |f| case(f, "keyed")["request_api_key"] = "placeholder-of-another".into(),
        Only("keyed"),
    ),
    (
        "a key its request does not write masked",
        |f| case(f, "keyed")["request"]["metadata"]["api_key"] = "".into(),
        Holding("failed: keyed\n  left: String(\"\")\n right: \"**********\""),
    ),
    (
        "what is no key written otherwise than the request carries it",
        |f| case(f, "numbered")["request_api_key"] = 8.into(),
        Holding("failed: numbered\n  left: Number(7)\n right: Number(8)"),
    ),
    (
        "a request the contract reads, listed as one it does not",
        |f| {
            let read = AiContext::default().written();
            case(f, "unread")["request"] = read;
        },
        Holding("left: []\n right: [\"unread\"]"),
    ),
    (
        "a request the contract does not read, and that no list names",
        |f| case(f, "answered")["request"] = json!({"messages": "none"}),
        Holding("left: [\"answered\", \"unread\"]\n right: [\"unread\"]"),
    ),
];
