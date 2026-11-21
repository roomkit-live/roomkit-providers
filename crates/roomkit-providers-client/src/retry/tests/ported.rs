//! Whether a failed try is tried again and after what wait, read through
//! what a provider calls: the wait before a try, and whether an answer is
//! worth one. Each test names the test it was ported from, in the crate the
//! rule was written from.

use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use super::TWO_MINUTES;

/// The clock a date is read against, in seconds since 1970:
/// 1970-01-12T13:46:40Z.
const NOW: f64 = 1_000_000.0;

/// The longest first backoff: a wait the headers do not give is at most it.
const FIRST_BACKOFF: Duration = Duration::from_millis(500);

fn should_retry(status: StatusCode, headers: &HeaderMap, now: f64) -> bool {
    TWO_MINUTES.should_retry(status, headers, now)
}

fn delay(retries_taken: u32, headers: Option<&HeaderMap>, now: f64) -> Duration {
    TWO_MINUTES.delay(retries_taken, headers, now)
}

fn headers(given: &[(&'static str, &'static str)]) -> HeaderMap {
    let named = given.iter().map(|(name, value)| {
        (
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        )
    });
    named.collect()
}

/// The wait before the first try again of an answer with the headers
/// `given`.
fn wait(given: &[(&'static str, &'static str)]) -> Duration {
    delay(0, Some(&headers(given)), NOW)
}

// Source: src/retry.rs::tests::a_wait_is_read_from_its_headers_in_the_sdk_s_order
// (the forms of a date no other test reads)
#[test]
fn a_wait_is_read_from_a_date_in_any_of_its_forms() {
    // Nine seconds past the clock, in HTTP's three forms, a wrong weekday
    // not held against it.
    for date in [
        "Mon, 12 Jan 1970 13:46:49 GMT",
        "Tue, 12 Jan 1970 13:46:49 GMT",
        "Monday, 12-Jan-70 13:46:49 GMT",
        "Mon Jan 12 13:46:49 1970",
        "12 Jan 1970 13:46:49 GMT",
        "Mon,12 Jan 1970 13:46:49 GMT",
        "12 Jan 1970 13:46:49 +0000",
    ] {
        assert_eq!(
            wait(&[("retry-after", date)]),
            Duration::from_secs(9),
            "{date}"
        );
    }
}

// Source: src/retry.rs::tests::a_wait_the_sdk_reads_and_this_crate_does_not
#[test]
fn a_wait_the_sdk_reads_and_this_crate_does_not() {
    // Python's `float()` reads an underscore between digits, in either
    // header. Unread, `1_000` seconds do not end the tries, as a wait past
    // two minutes would, and `1_000` milliseconds leave the backoff.
    let status = StatusCode::TOO_MANY_REQUESTS;
    assert!(should_retry(
        status,
        &headers(&[("retry-after", "1_000")]),
        NOW
    ));
    assert!(wait(&[("retry-after-ms", "1_000")]) <= FIRST_BACKOFF);
    // The next header is read instead, where the SDK waits one second.
    assert_eq!(
        wait(&[("retry-after-ms", "1_000"), ("retry-after", "9")]),
        Duration::from_secs(9)
    );
    // `parsedate_tz` reads a date with no time zone, and a four-digit year
    // in the obsolete form: each nine seconds past the clock, were it read.
    for date in ["Mon, 12 Jan 1970 13:46:49", "Mon, 12-Jan-1970 13:46:49 GMT"] {
        assert!(wait(&[("retry-after", date)]) <= FIRST_BACKOFF, "{date}");
    }
}

// Source: src/retry.rs::tests::a_status_is_worth_a_retry_as_the_sdk_reads_it
// (the rows no other test holds)
#[test]
fn a_status_is_worth_a_retry_as_the_sdk_reads_it() {
    let worth = |status: u16, given: &[(&'static str, &'static str)]| {
        should_retry(StatusCode::from_u16(status).unwrap(), &headers(given), NOW)
    };
    for status in [599, 600] {
        assert!(worth(status, &[]), "{status}");
    }
    // Only `true` and `false` are read: any other value leaves the status
    // to decide.
    assert!(worth(503, &[("x-should-retry", "TRUE")]));
    // A wait that is no number of seconds ends nothing.
    assert!(worth(429, &[("retry-after-ms", "nan")]));
}

// Source: src/retry.rs::tests::a_wait_s_jitter_stays_within_a_unit and
// ::the_backoff_doubles_from_half_a_second_to_eight (its last row)
#[test]
fn a_backoff_is_taken_whole_or_up_to_a_quarter_less() {
    let within = |retries_taken: u32, most: Duration| {
        let wait = delay(retries_taken, None, NOW);
        most.mul_f64(0.75) <= wait && wait <= most
    };
    for _ in 0..100 {
        assert!(within(0, FIRST_BACKOFF));
    }
    // The backoff stops doubling at eight seconds, however many tries.
    assert!(within(u32::MAX, Duration::from_secs(8)));
}
