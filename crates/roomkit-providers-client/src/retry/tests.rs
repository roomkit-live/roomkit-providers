//! The rule read as the SDK that gives up past a wait of two minutes reads
//! it, then as the one that waits the longest it takes.

use super::*;

mod ported;

/// 2026-01-01T00:00:00Z.
const NOW: f64 = 1_767_225_600.0;

/// The rule of an SDK that takes a wait of two minutes at most, and gives
/// the request up past it.
const TWO_MINUTES: Rule = Rule {
    longest_asked: 120.0,
    past_it: Past::Final,
};

/// The rule of an SDK that takes any wait, up to the longest a timer
/// counts.
const ANY_WAIT: Rule = Rule {
    longest_asked: 4_294_967.0,
    past_it: Past::Longest,
};

fn should_retry(status: StatusCode, headers: &HeaderMap, now: f64) -> bool {
    TWO_MINUTES.should_retry(status, headers, now)
}

fn delay_within(retries_taken: u32, asked: Option<f64>, random: f64) -> Duration {
    TWO_MINUTES.delay_within(retries_taken, asked, random)
}

fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
    let named = pairs.iter().map(|(name, value)| {
        let name = reqwest::header::HeaderName::from_static(name);
        (name, HeaderValue::from_static(value))
    });
    named.collect()
}

#[test]
fn a_wait_is_what_the_answer_asks_within_two_minutes_else_a_backoff() {
    let asked = |pairs: &[(&'static str, &'static str)]| retry_after(&headers(pairs), NOW);
    assert_eq!(asked(&[("retry-after", "7")]), Some(7.0));
    assert_eq!(asked(&[("retry-after", " 1.5 ")]), Some(1.5));
    assert_eq!(
        asked(&[("retry-after-ms", "1500"), ("retry-after", "7")]),
        Some(1.5)
    );
    assert_eq!(
        asked(&[("retry-after-ms", "soon"), ("retry-after", "3")]),
        Some(3.0)
    );
    for date in [
        "Thu, 01 Jan 2026 00:00:07 GMT",
        "Thursday, 01-Jan-26 00:00:07 GMT",
        "Thu Jan  1 00:00:07 2026",
    ] {
        assert_eq!(asked(&[("retry-after", date)]), Some(7.0), "{date}");
    }
    assert_eq!(asked(&[("retry-after", "soon")]), None);
    assert_eq!(asked(&[]), None);

    let wait = |taken, asked, random| delay_within(taken, asked, random).as_secs_f64();
    assert!((wait(0, Some(7.0), 0.5) - 7.0).abs() < 1e-9);
    assert!((wait(0, Some(120.0), 0.5) - 120.0).abs() < 1e-9);
    for unreasonable in [0.0, -5.0, 121.0, f64::INFINITY, f64::NAN] {
        assert!((wait(0, Some(unreasonable), 0.0) - 0.5).abs() < 1e-9);
    }
    let backoffs: Vec<f64> = (0..6).map(|taken| wait(taken, None, 0.0)).collect();
    assert_eq!(backoffs, [0.5, 1.0, 2.0, 4.0, 8.0, 8.0]);
    assert!((wait(1, None, 1.0) - 0.75).abs() < 1e-9);
}

#[test]
fn an_answer_is_tried_again_as_its_headers_say_else_as_its_status_does() {
    let again = |status: u16, pairs: &[(&'static str, &'static str)]| {
        should_retry(StatusCode::from_u16(status).unwrap(), &headers(pairs), NOW)
    };
    for status in [408, 409, 429, 500, 502, 503, 504, 529] {
        assert!(again(status, &[]), "{status}");
    }
    for status in [400, 401, 403, 404, 413, 422] {
        assert!(!again(status, &[]), "{status}");
    }
    assert!(again(400, &[("x-should-retry", "true")]));
    assert!(!again(400, &[("x-should-retry", "TRUE")]));
    assert!(!again(503, &[("x-should-retry", "false")]));
    assert!(again(429, &[("retry-after", "120")]));
    assert!(!again(429, &[("retry-after", "121")]));
    assert!(!again(
        429,
        &[("retry-after", "121"), ("x-should-retry", "true")]
    ));
}

// What Python's SDK reads of a `retry-after` and this crate does not, or
// reads otherwise: each is written in the README of a provider.
#[test]
fn a_retry_after_python_s_sdk_reads_otherwise() {
    let asked =
        |name: &'static str, value: &'static str| retry_after(&headers(&[(name, value)]), NOW);
    // A number too large for a float is infinite: no wait is taken from
    // it, and the status decides, where the SDK gives the request up.
    for huge in [
        "1e400",
        "99999999999999999999999999999999999999999999999999e300",
    ] {
        assert_eq!(asked("retry-after", huge), Some(f64::INFINITY));
        assert_eq!(asked("retry-after-ms", huge), Some(f64::INFINITY));
    }
    let limited = StatusCode::TOO_MANY_REQUESTS;
    assert!(should_retry(
        limited,
        &headers(&[("retry-after", "1e400")]),
        NOW
    ));
    let wait = delay_within(0, Some(f64::INFINITY), 0.0);
    assert_eq!(wait, Duration::from_millis(500));
    // Digits with underscores are no number here: the backoff. The SDK
    // reads 10 s and 1 s.
    assert_eq!(asked("retry-after", "1_0"), None);
    assert_eq!(asked("retry-after-ms", "1_000"), None);
    // A date with no zone, with single-digit fields or with a day past
    // its month is no date here: the backoff. The SDK reads the first
    // as UTC, the second as written, and the third as the first day of
    // the next month, too far ahead to wait for.
    for date in [
        "Thu, 01 Jan 2026 00:00:07",
        "Thu, 1 Jan 2026 0:0:7 GMT",
        "Thu, 32 Jan 2026 00:00:07 GMT",
    ] {
        assert_eq!(asked("retry-after", date), None, "{date}");
    }
}

// A rule that takes any wait tries again after what the answer asks, however
// long, up to its longest; a wait that is no number above zero leaves the
// backoff, as under the other rule.
#[test]
fn a_rule_that_takes_any_wait_waits_its_longest_at_most() {
    let wait = |asked: f64| ANY_WAIT.delay_within(0, Some(asked), 0.0).as_secs_f64();
    for asked in [7.0, 121.0, 86_400.0] {
        assert!((wait(asked) - asked).abs() < 1e-9, "{asked}");
    }
    for huge in [5_000_000.0, f64::INFINITY] {
        assert!((wait(huge) - 4_294_967.0).abs() < 1e-9, "{huge}");
    }
    for unreasonable in [0.0, -5.0, f64::NAN] {
        assert!((wait(unreasonable) - 0.5).abs() < 1e-9, "{unreasonable}");
    }
    // An answer that asks for more than two minutes is still worth a try,
    // and `x-should-retry` and the status decide as under the other rule.
    let again = |status: u16, pairs: &[(&'static str, &'static str)]| {
        ANY_WAIT.should_retry(StatusCode::from_u16(status).unwrap(), &headers(pairs), NOW)
    };
    assert!(again(429, &[("retry-after", "121")]));
    assert!(again(429, &[("retry-after", "1e400")]));
    assert!(!again(503, &[("x-should-retry", "false")]));
    assert!(again(400, &[("x-should-retry", "true")]));
    assert!(!again(404, &[("retry-after", "121")]));
}
