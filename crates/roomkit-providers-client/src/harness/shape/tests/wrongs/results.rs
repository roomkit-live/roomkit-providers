//! What Python gave of each case: its result as the contract reads it, its
//! error, how a replay compares it, and the cases the fixture questions.

use serde_json::json;

use super::super::Says::{Holding, Only};
use super::super::Wrong;
use super::super::sound::provider_error;
use super::{backwards, case, error, without, without_its_last};

/// `each_result_reads_as_the_contract_s`: what each call gives, and what
/// reads as the contract's types.
pub(super) const RESULTS: &[Wrong] = &[
    (
        "a text beside the response of a generation",
        |f| case(f, "answered")["expected"]["text"] = json!(["Yes."]),
        Holding("failed: answered\n  left: [true, true, false, false]"),
    ),
    (
        "events beside the text of a stream of text",
        |f| case(f, "streamed_as_text")["expected"]["events"] = json!([]),
        Only("streamed_as_text"),
    ),
    (
        "a text beside the events of a stream",
        |f| case(f, "streamed")["expected"]["text"] = json!([]),
        Only("streamed"),
    ),
    (
        "a response beside the models of a listing",
        |f| case(f, "listed")["expected"]["response"] = json!({}),
        Holding("failed: listed\n  left: [true, false, false, true]"),
    ),
    (
        "a call no provider has",
        |f| case(f, "answered")["call"] = "embed".into(),
        Only("answered: an unknown call embed"),
    ),
    (
        "a response that is none of the contract's",
        |f| case(f, "answered")["expected"]["response"]["content"] = 7.into(),
        Holding("answered: invalid type: integer `7`, expected a string"),
    ),
    (
        "a response whose members come in another order",
        |f| backwards(&mut case(f, "answered")["expected"]["response"]),
        Holding("failed: answered\n  left: Some(\"{\\\"content\\\""),
    ),
    (
        "a model that is none of the contract's",
        |f| case(f, "listed")["expected"]["models"][0]["id"] = 7.into(),
        Holding("listed: invalid type: integer `7`, expected a string"),
    ),
    (
        "an event that is none of the contract's",
        |f| case(f, "streamed")["expected"]["events"][0]["type"] = "unheard".into(),
        Holding("streamed: "),
    ),
];

/// `each_result_reads_as_the_contract_s`: what an error reads as.
pub(super) const ERRORS: &[Wrong] = &[
    (
        "a ProviderError that does not say whether it came before a request",
        |f| without(error(f, "refused"), "before_request"),
        Only("refused: a ProviderError without before_request"),
    ),
    (
        "a ProviderError that does not say whether a new request may succeed",
        |f| without(error(f, "refused"), "retryable"),
        Only("refused: a ProviderError without retryable"),
    ),
    (
        "an error of a provider that is of another class",
        |f| error(f, "refused")["class"] = "VendorRefusal".into(),
        Holding("failed: refused\n  left: \"VendorRefusal\"\n right: \"ProviderError\""),
    ),
    (
        "a reason of a class that has none",
        |f| error(f, "off_the_schema")["class"] = "ProviderError".into(),
        Holding(
            "failed: off_the_schema\n  left: \"ProviderError\"\n right: \"ResponseSchemaError\"",
        ),
    ),
    (
        "a reason the contract does not name",
        |f| error(f, "off_the_schema")["reason"] = "unheard".into(),
        Only("off_the_schema: a ProviderError without a reason the contract names"),
    ),
    (
        "a response schema's failure worth a new request",
        |f| error(f, "off_the_schema")["retryable"] = true.into(),
        Holding(
            "failed: off_the_schema\n  left: (false, false, None, None)\n right: (true, false, None, None)",
        ),
    ),
    (
        "a ProviderError of no provider",
        |f| error(f, "raw")["class"] = "ProviderError".into(),
        Holding("`left != right` failed: raw\n  left: \"ProviderError\""),
    ),
    (
        "a response schema's failure of no provider",
        |f| error(f, "raw")["class"] = "ResponseSchemaError".into(),
        Holding("`left != right` failed: raw\n  left: \"ResponseSchemaError\""),
    ),
    (
        "a text that is not the contract's",
        |f| error(f, "refused")["text"] = "the key is refused".into(),
        Holding(
            "failed: refused\n  left: \"vendor (401): the key is refused\"\n right: \"the key is refused\"",
        ),
    ),
    (
        "another provider's error",
        |f| {
            *error(f, "refused") =
                provider_error("another", "the key is refused", Some(401), false, false);
        },
        Holding("failed: refused\n  left: \"another\"\n right: \"vendor\""),
    ),
    (
        "a failure before any request that had an exchange",
        |f| {
            let asked = case(f, "answered")["exchanges"].clone();
            case(f, "unbuilt")["exchanges"] = asked;
        },
        Only("unbuilt"),
    ),
    (
        "an error raised from a class with no name",
        |f| error(f, "refused")["raised_from"] = "".into(),
        Only("refused: an empty class"),
    ),
];

/// `each_failure_says_what_a_replay_reproduces`.
pub(super) const COMPARED: &[Wrong] = &[
    (
        "a failure that does not say how it is compared",
        |f| without(case(f, "refused"), "compare"),
        Holding("failed: refused\n  left: false\n right: true"),
    ),
    (
        "an answer that says how its failure is compared",
        |f| case(f, "answered")["compare"] = json!({"exactly": [], "message": "roomkit"}),
        Holding("failed: answered\n  left: true\n right: false"),
    ),
    (
        "a server's words given as RoomKit's",
        |f| case(f, "refused")["compare"]["message"] = "roomkit".into(),
        Holding("failed: refused\n  left: \"roomkit\"\n right: \"server\""),
    ),
    (
        "a raw exception a replay would reproduce",
        |f| case(f, "raw")["compare"]["exactly"] = json!(["class"]),
        Only("raw"),
    ),
    (
        "a failure that leaves out an attribute",
        |f| without_its_last(&mut case(f, "refused")["compare"]["exactly"]),
        Holding(
            "failed: refused\n  left: [\"class\", \"status_code\", \"retryable\", \"provider\", \"context_overflow\"]",
        ),
    ),
    (
        "a response schema's failure that leaves out its reason",
        |f| without_its_last(&mut case(f, "off_the_schema")["compare"]["exactly"]),
        Holding("failed: off_the_schema\n  left: ["),
    ),
    (
        "a stream's failure that leaves out its events",
        |f| without_its_last(&mut case(f, "lost_under_way")["compare"]["exactly"]),
        Holding("failed: lost_under_way\n  left: ["),
    ),
    (
        "a failure before any request in the SDK's words",
        |f| {
            let said = provider_error("vendor", "Connection error.", None, false, true);
            *error(f, "unbuilt") = said;
            case(f, "unbuilt")["compare"]["message"] = "sdk".into();
            case(f, "unbuilt")["sdk"] = "The SDK words the failure.".into();
        },
        Holding("failed: unbuilt\n  left: \"sdk\"\n right: \"roomkit\""),
    ),
];

/// `the_questioned_cases_are`.
pub(super) const QUESTIONED: &[Wrong] = &[
    (
        "a questioned case the test does not list",
        |f| case(f, "answered")["questioned"] = "A possible defect: it answers.".into(),
        Holding("left: [\"answered\", \"redirected\", \"to_decide\"]"),
    ),
    (
        "a list of the questioned cases that leaves one out",
        |f| f["questioned"] = json!(["redirected"]),
        Holding("left: [\"redirected\"]\n right: [\"redirected\", \"to_decide\"]"),
    ),
    (
        "a question that is no sentence",
        |f| case(f, "redirected")["questioned"] = "A possible defect: it is followed".into(),
        Only("redirected"),
    ),
    (
        "a question of neither kind",
        |f| case(f, "to_decide")["questioned"] = "Something to look at.".into(),
        Only("to_decide"),
    ),
    (
        "a possible defect given as a difference to decide",
        |f| case(f, "redirected")["questioned"] = "A difference to decide: it is followed.".into(),
        Holding("left: [\"redirected\", \"to_decide\"]\n right: [\"to_decide\"]"),
    ),
    (
        "a note that does not name the possible defects",
        |f| f["notes"]["questioned"] = "A difference to decide.".into(),
        Holding("contains(POSSIBLE_DEFECT"),
    ),
    (
        "a note that does not name the differences to decide",
        |f| f["notes"]["questioned"] = "A possible defect.".into(),
        Holding("contains(TO_DECIDE"),
    ),
];
