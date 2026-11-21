//! Whether a failed try is tried again, and after what wait: the rules of
//! Python's openai SDK, which `max_retries` configures there.

use std::hash::{BuildHasher, Hasher};
use std::time::Duration;

use jiff::Timestamp;
use jiff::fmt::rfc2822::DateTimeParser;
use jiff::fmt::strtime::BrokenDownTime;
use jiff::tz::TimeZone;
use reqwest::StatusCode;
use reqwest::header::{HeaderMap, HeaderValue};

/// The first backoff, which doubles with each try up to the longest.
const INITIAL_RETRY_DELAY: f64 = 0.5;
const MAX_RETRY_DELAY: f64 = 8.0;
/// The longest wait a server may ask for: past it the answer is final.
const MAX_RETRY_AFTER_DELAY: f64 = 120.0;

/// Whether an answer of `status` with `headers` is worth another try at
/// `now`, in seconds since 1970: what `x-should-retry` says, else a 408, a
/// 409, a 429 or a server error; never past a wait of two minutes.
pub(crate) fn should_retry(status: StatusCode, headers: &HeaderMap, now: f64) -> bool {
    let asked = retry_after(headers, now);
    if asked.is_some_and(|wait| wait.is_finite() && wait > MAX_RETRY_AFTER_DELAY) {
        return false;
    }
    match headers.get("x-should-retry").map(HeaderValue::as_bytes) {
        Some(b"true") => return true,
        Some(b"false") => return false,
        _ => {}
    }
    let status = status.as_u16();
    matches!(status, 408 | 409 | 429) || status >= 500
}

/// The wait before the try after `retries_taken`: what the answer's
/// `headers` ask, within two minutes, else 0.5 s doubling to 8 s, up to a
/// quarter less.
pub(crate) fn delay(retries_taken: u32, headers: Option<&HeaderMap>, now: f64) -> Duration {
    let asked = headers.and_then(|headers| retry_after(headers, now));
    delay_within(retries_taken, asked, random_unit())
}

fn delay_within(retries_taken: u32, asked: Option<f64>, random: f64) -> Duration {
    let reasonable = |wait: &f64| wait.is_finite() && *wait > 0.0 && *wait <= MAX_RETRY_AFTER_DELAY;
    let seconds = asked.filter(reasonable).unwrap_or_else(|| {
        let tries = i32::try_from(retries_taken.min(1000)).unwrap_or(1000);
        let backoff = INITIAL_RETRY_DELAY * 2.0_f64.powi(tries);
        backoff.min(MAX_RETRY_DELAY) * (1.0 - 0.25 * random)
    });
    Duration::try_from_secs_f64(seconds).unwrap_or(Duration::ZERO)
}

/// The wait the headers ask, in seconds: `retry-after-ms`, then
/// `retry-after` as a number, then as an HTTP date read against `now`.
fn retry_after(headers: &HeaderMap, now: f64) -> Option<f64> {
    let text = |name: &str| headers.get(name)?.to_str().ok();
    if let Some(millis) = text("retry-after-ms").and_then(python_float) {
        return Some(millis / 1000.0);
    }
    let after = text("retry-after")?;
    python_float(after).or_else(|| Some(date_seconds(after)? - now))
}

fn python_float(text: &str) -> Option<f64> {
    text.trim().parse().ok()
}

/// The seconds since 1970 of an HTTP date, in any of its three forms.
#[expect(
    clippy::cast_precision_loss,
    reason = "a date's seconds since the epoch are far within an f64's exact integers"
)]
fn date_seconds(text: &str) -> Option<f64> {
    let text = text.trim();
    let dated = text
        .split_once(',')
        .map_or(text, |(_, dated)| dated.trim_start());
    let date = DateTimeParser::new()
        .parse_timestamp(dated)
        .ok()
        .or_else(|| obsolete_date("%d-%b-%y %H:%M:%S GMT", dated))
        .or_else(|| {
            // `Sun Nov  6 08:49:37 1994`.
            let (_, dated) = text.split_once(' ')?;
            obsolete_date("%b %e %H:%M:%S %Y", dated)
        });
    Some(date?.as_second() as f64)
}

fn obsolete_date(format: &str, text: &str) -> Option<Timestamp> {
    let written = BrokenDownTime::parse(format, text)
        .ok()?
        .to_datetime()
        .ok()?;
    TimeZone::UTC.to_timestamp(written).ok()
}

/// A number from 0 to 1, drawn without a dependency for it.
fn random_unit() -> f64 {
    let bits = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    let high = u32::try_from(bits >> 32).unwrap_or(u32::MAX);
    f64::from(high) / f64::from(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-01-01T00:00:00Z.
    const NOW: f64 = 1_767_225_600.0;

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
    // reads otherwise: each is written in the crate's README.
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
}
