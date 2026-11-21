//! What a response schema's checks say when they refuse: Python's words of
//! `schema_for_generate` and `check_schema_answer`.

use serde_json::json;

use super::*;
use crate::tools::AiTool;

/// A schema of one required text.
fn schema() -> Map<String, Value> {
    json!({
        "type": "object",
        "properties": { "label": { "type": "string" } },
        "required": ["label"],
        "additionalProperties": false,
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}

/// A request under the schema, with a tool or none.
fn asking(with_tool: bool) -> AiRequest {
    let tools = if with_tool {
        vec![AiTool::new("lookup", "Look it up").unwrap()]
    } else {
        Vec::new()
    };
    AiRequest {
        response_schema: Some(schema()),
        tools,
        ..AiRequest::default()
    }
}

/// What a failure says: its reason, whether it came before any request,
/// and its words.
fn said(error: &ProviderError) -> (Option<ResponseSchemaFailure>, bool, &str) {
    let reason = error
        .source()
        .and_then(|source| source.downcast_ref::<ResponseSchemaError>())
        .map(|failure| failure.reason);
    (reason, error.before_request, error.message.as_str())
}

// Python: schema_for_generate(context, supported=, with_tools=): a schema is
// carried beside tools only by a provider that takes both.
#[test]
fn a_schema_beside_tools_is_carried_only_by_a_provider_that_takes_both() {
    let unsupported = Some(ResponseSchemaFailure::Unsupported);
    assert_eq!(
        schema_for_generate(&asking(true), true, true, "acme"),
        Ok(Some(&schema()))
    );
    assert_eq!(
        schema_for_generate(&asking(false), true, false, "acme"),
        Ok(Some(&schema()))
    );
    let beside_tools = schema_for_generate(&asking(true), true, false, "acme").unwrap_err();
    assert_eq!(
        said(&beside_tools),
        (
            unsupported,
            true,
            "acme cannot combine a response schema with tools in the same turn; check \
             supports_response_schema_with_tools before setting both"
        )
    );
    // A provider that takes no schema says so first, tools or not.
    for with_tool in [false, true] {
        let none = schema_for_generate(&asking(with_tool), false, true, "acme").unwrap_err();
        assert_eq!(
            said(&none),
            (
                unsupported,
                true,
                "acme does not support a response schema; check supports_response_schema \
                 before setting one"
            )
        );
        assert!(!none.retryable);
    }
}

// Python: check_schema_answer: a refusal first, then an answer cut short,
// then a text that is no document, then a document of another shape.
#[test]
fn an_answer_without_its_document_says_why() {
    let checked = |content: &str, refusal: Option<&str>, finish: Option<&str>| {
        check_schema_answer(content, &schema(), "acme", refusal, finish)
    };
    assert_eq!(checked(r#"{"label": "yes"}"#, None, Some("stop")), Ok(()));
    assert_eq!(checked(r#"{"label": "yes"}"#, None, None), Ok(()));

    let declined = checked(r#"{"label": "y"#, Some("unsafe request"), Some("length")).unwrap_err();
    assert_eq!(
        said(&declined),
        (
            Some(ResponseSchemaFailure::Refusal),
            false,
            "the model declined to answer: unsafe request"
        )
    );
    let cut = checked(r#"{"label": "y"#, None, Some("MAX_TOKENS")).unwrap_err();
    assert_eq!(
        said(&cut),
        (
            Some(ResponseSchemaFailure::Truncated),
            false,
            "the JSON answer was cut (MAX_TOKENS): raise max_tokens, or shorten the context"
        )
    );
    let prose = checked("Sure, the answer is yes.", None, Some("stop")).unwrap_err();
    let (reason, before_request, words) = said(&prose);
    assert_eq!(
        (reason, before_request),
        (Some(ResponseSchemaFailure::InvalidJson), false)
    );
    assert!(
        words.starts_with("the answer is not a JSON document (") && words.ends_with(')'),
        "{words}"
    );
    let other = checked(r#"{"verdict": "yes"}"#, None, Some("stop")).unwrap_err();
    assert_eq!(
        said(&other),
        (
            Some(ResponseSchemaFailure::InvalidJson),
            false,
            "the answer does not satisfy the response schema ($: missing ['label'], unexpected \
             ['verdict'])"
        )
    );
    assert_eq!(other.to_string(), format!("acme: {}", other.message));
}

// Python: ResponseSchemaFailure's four values.
#[test]
fn a_failure_s_reason_is_named_as_python_names_it() {
    let named = [
        ResponseSchemaFailure::Unsupported,
        ResponseSchemaFailure::Refusal,
        ResponseSchemaFailure::Truncated,
        ResponseSchemaFailure::InvalidJson,
    ]
    .map(ResponseSchemaFailure::as_str);
    assert_eq!(
        named,
        ["unsupported", "refusal", "truncated", "invalid_json"]
    );
}
