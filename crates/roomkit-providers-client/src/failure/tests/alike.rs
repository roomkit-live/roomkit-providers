//! The failures every provider words alike: what each says, whether a new
//! request may succeed, what it repeats of an answer, and which status an
//! error body names.

use roomkit_providers_http::SseError;
use serde_json::{Value, json};

use super::{KEY, STREAM_ERROR, test_exit};
use crate::failure::{
    Failure, MESSAGE_LIMIT, NAME_LIMIT, Stated, described_status, past_mark, retryable_status,
};

/// What `failure` says once it left, whether a new request may succeed,
/// and its status.
fn read(failure: &Failure) -> (String, bool, Option<u16>) {
    let error = test_exit().leave(failure);
    (error.message, error.retryable, error.status_code)
}

// An answer that does not read is final, whatever in it does not: no event,
// a stream past what is read of one, a body that is no JSON, a body of
// another format, a value the reader refuses. What a reader quotes of the
// answer is cleaned of the key, and a value is bounded as a name is.
#[test]
fn an_answer_that_does_not_read_is_final_and_repeats_no_secret() {
    let none = "The provider's stream carried no event: its body could not be read";
    assert_eq!(read(&Failure::no_event()), (none.to_owned(), false, None));
    let too_long = SseError::StreamLineTooLong { limit: 8 };
    let unread = read(&Failure::stream_unread(&too_long));
    assert_eq!(unread, (too_long.to_string(), false, None));
    let quoting = serde_json::from_str::<u8>(&format!("\"{KEY}\"")).unwrap_err();
    let malformed = read(&Failure::malformed(&quoting));
    let said = "invalid type: string \"[***]\", expected u8 at line 1 column 23";
    assert_eq!(malformed, (said.to_owned(), false, None));
    let unreadable = read(&Failure::unreadable("it holds no choice"));
    let said = "The provider answered with a body it could not read: it holds no choice";
    assert_eq!(unreadable, (said.to_owned(), false, None));
    let value = format!("{KEY} {}", "x".repeat(2 * NAME_LIMIT));
    let refused = read(&Failure::unreadable_value("the finish reason", value));
    let kept = "x".repeat(NAME_LIMIT - "[***] ".len());
    let said = format!(
        "The provider answered with a body it could not read: the finish reason: [***] {kept}"
    );
    assert_eq!(refused, (said, false, None));
}

// A refusal bounds what it repeats of an error's type as a name, and says
// that the context overflowed when the provider's code says so, or when
// the words of the body do, and not otherwise.
#[test]
fn a_refusal_bounds_its_names_and_reads_an_overflow_from_a_code_or_from_words() {
    let named = Stated {
        kind: Some("k".repeat(2 * NAME_LIMIT)),
        message: Some("m".repeat(2 * MESSAGE_LIMIT)),
        ..Stated::default()
    };
    let (said, ..) = read(&Failure::refused(418, b"{}", named));
    let expected = format!(
        "the provider answered HTTP 418 ({}): {}",
        "k".repeat(NAME_LIMIT),
        "m".repeat(MESSAGE_LIMIT)
    );
    assert_eq!(
        said,
        expected.chars().take(MESSAGE_LIMIT).collect::<String>()
    );
    let overflowed = |body: &str, overflow: bool| {
        let stated = Stated {
            overflow,
            ..Stated::default()
        };
        let failure = Failure::refused(400, body.as_bytes(), stated);
        test_exit().leave(&failure).context_overflow
    };
    let in_words = "This model's maximum context length is 8 tokens";
    assert_eq!(overflowed("{}", true), Some(true));
    assert_eq!(overflowed(in_words, false), Some(true));
    assert_eq!(overflowed("{}", false), None);
}

// An error inside a stream that brings an empty message says what Python's
// client says of one that brings none.
#[test]
fn an_error_inside_a_stream_with_an_empty_message_reads_as_one_with_none() {
    let empty = Failure::stream_error(&json!({"message": "", "code": 503}), STREAM_ERROR);
    assert_eq!(read(&empty), (STREAM_ERROR.to_owned(), true, Some(503)));
}

// Python: providers/ai/base.py _RETRYABLE_STATUS: a timeout, a conflict, a
// rate limit and any server error, and no other status.
#[test]
fn a_status_is_retried_when_it_is_transient() {
    for retried in [408, 409, 429, 500, 503, 529, 599] {
        assert!(retryable_status(retried), "{retried}");
    }
    for last in [400, 401, 404, 407, 410, 428, 430, 499, 600, 700] {
        assert!(!retryable_status(last), "{last}");
    }
}

// Python: providers/ai/base.py _ERROR_TYPE_STATUS and _described_status:
// the status an error's type names, an error status written as a number or
// in digits, read on the innermost error object, under `error` or `data`,
// and in a body that is a text of JSON.
#[test]
fn an_error_body_names_its_status_by_a_number_or_by_its_type() {
    let named = [
        ("invalid_request_error", 400),
        ("authentication_error", 401),
        ("permission_error", 403),
        ("not_found_error", 404),
        ("request_too_large", 413),
        ("rate_limit_error", 429),
        ("rate_limit_exceeded", 429),
        ("api_error", 500),
        ("server_error", 500),
        ("internal_error", 500),
        ("service_unavailable", 503),
        ("timeout_error", 504),
        ("overloaded_error", 529),
        ("INVALID_ARGUMENT", 400),
        ("UNAUTHENTICATED", 401),
        ("PERMISSION_DENIED", 403),
        ("NOT_FOUND", 404),
        ("RESOURCE_EXHAUSTED", 429),
        ("INTERNAL", 500),
        ("UNAVAILABLE", 503),
        ("DEADLINE_EXCEEDED", 504),
    ];
    for (name, status) in named {
        assert_eq!(
            described_status(&json!({"type": name})),
            Some(status),
            "{name}"
        );
    }
    let described = |body: Value| described_status(&body);
    assert_eq!(described(json!({"code": 400})), Some(400));
    assert_eq!(described(json!({"code": "599"})), Some(599));
    for no_error in [
        json!(399),
        json!(600),
        json!("399"),
        json!("600"),
        json!(200),
    ] {
        assert_eq!(described(json!({"code": no_error})), None);
    }
    assert_eq!(described(json!({"data": {"status": 503}})), Some(503));
    assert_eq!(described(json!("{\"error\": {\"code\": 429}}")), Some(429));
    assert_eq!(described(json!("no JSON")), None);
    let under_data = Failure::error_object(&json!({"data": {"message": "inner", "code": 429}}));
    assert_eq!(read(&under_data), ("inner".to_owned(), true, Some(429)));
}

// A body is read past the byte order mark it may start with, as Python's
// JSON reader takes one before a document.
#[test]
fn a_body_is_read_past_its_byte_order_mark() {
    assert_eq!(past_mark(b"\xEF\xBB\xBF{}"), b"{}");
    assert_eq!(past_mark(b"{}"), b"{}");
    assert_eq!(past_mark(b"{}\xEF\xBB\xBF"), b"{}\xEF\xBB\xBF");
}
