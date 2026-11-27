//! Whether a failed try is tried again, and after what wait: the rules of
//! the SDK under Python's provider, which `max_retries` configures there.
//!
//! The SDKs of the vendors share these rules but for the longest wait a
//! server may ask for and what a longer one does: a provider states both in
//! its [`Rule`].

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

/// The waits of an SDK that tries again only a request that got no answer
/// (a lost connection, a timeout), never one a server answered whatever its
/// status: the first wait, doubled at each try, with no part drawn at
/// random. A provider whose SDK does so says it with
/// [`Client::try_again_only_unanswered`](crate::Client::try_again_only_unanswered).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Backoff {
    /// The wait before the second try, in seconds.
    pub first: f64,
}

impl Backoff {
    /// The wait before the try after `retries_taken`.
    #[must_use]
    pub fn delay(&self, retries_taken: u32) -> Duration {
        let tries = i32::try_from(retries_taken.min(1000)).unwrap_or(1000);
        let seconds = self.first * 2.0_f64.powi(tries);
        Duration::try_from_secs_f64(seconds).unwrap_or(Duration::MAX)
    }
}

/// What an answer that asks for a wait past the longest of a [`Rule`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Past {
    /// Final: no other try comes, whatever its status. A wait that is no
    /// finite number is not taken, and the backoff stands in its place.
    Final,
    /// Tried again after the longest wait, an infinite one included.
    Longest,
}

/// Where the retry rules of two SDKs differ.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rule {
    /// The longest wait a server's answer may ask for, in seconds.
    pub longest_asked: f64,
    /// What an answer that asks for a longer wait is.
    pub past_it: Past,
}

impl Rule {
    /// Whether an answer of `status` with `headers` is worth another try
    /// at `now`, in seconds since 1970: what `x-should-retry` says, else a
    /// 408, a 409, a 429 or a server error; and never, for a rule whose
    /// longest wait is [`Past::Final`], past that wait.
    #[must_use]
    pub fn should_retry(&self, status: StatusCode, headers: &HeaderMap, now: f64) -> bool {
        let asked = retry_after(headers, now);
        let too_long = |wait: f64| wait.is_finite() && wait > self.longest_asked;
        if self.past_it == Past::Final && asked.is_some_and(too_long) {
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
    /// `headers` ask, as far as the rule takes it, else 0.5 s doubling to
    /// 8 s, up to a quarter less.
    #[must_use]
    pub fn delay(&self, retries_taken: u32, headers: Option<&HeaderMap>, now: f64) -> Duration {
        let asked = headers.and_then(|headers| retry_after(headers, now));
        self.delay_within(retries_taken, asked, random_unit())
    }

    /// The wait `asked` as the rule takes it: none for one that is not
    /// above zero.
    fn taken(&self, asked: f64) -> Option<f64> {
        let above_zero = asked > 0.0;
        match self.past_it {
            Past::Final => Some(asked)
                .filter(|wait| above_zero && wait.is_finite() && *wait <= self.longest_asked),
            Past::Longest => above_zero.then(|| asked.min(self.longest_asked)),
        }
    }

    fn delay_within(&self, retries_taken: u32, asked: Option<f64>, random: f64) -> Duration {
        let seconds = asked
            .and_then(|asked| self.taken(asked))
            .unwrap_or_else(|| {
                let tries = i32::try_from(retries_taken.min(1000)).unwrap_or(1000);
                let backoff = INITIAL_RETRY_DELAY * 2.0_f64.powi(tries);
                backoff.min(MAX_RETRY_DELAY) * (1.0 - 0.25 * random)
            });
        Duration::try_from_secs_f64(seconds).unwrap_or(Duration::ZERO)
    }
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
mod tests;
