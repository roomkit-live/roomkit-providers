//! The timeouts a client is built with.

use std::time::{Duration, Instant};

use super::Timeouts;
use crate::testing::left;

// A timeout is refused while the clock could not count a hundred years past
// its end: the longest the clock takes now is not one a client is built
// with, since the clock would fail on it a moment later.
#[test]
fn a_timeout_leaves_the_clock_a_hundred_years() {
    let counted = |seconds: u64| {
        let timeout = Duration::from_secs(seconds);
        Instant::now().checked_add(timeout).is_some()
    };
    // The longest timeout the clock counts now, found by halving.
    let (mut longest, mut too_long) = (0, u64::MAX);
    assert!(counted(longest) && !counted(too_long));
    while too_long - longest > 1 {
        let middle = longest + (too_long - longest) / 2;
        if counted(middle) {
            longest = middle;
        } else {
            too_long = middle;
        }
    }
    let years = |years: u64| years * 365 * 24 * 3600;
    let built = |seconds: u64, member: &str| {
        let timeout = Duration::from_secs(seconds);
        let usual = Duration::from_secs(30);
        let timeouts = match member {
            "timeout" => Timeouts::new(timeout, usual),
            _ => Timeouts::new(usual, timeout),
        };
        timeouts
            .map(|_| ())
            .map_err(|failure| left(&failure).message)
    };
    let refused = "the provider cannot be built: its timeout is longer than the clock can count";
    for member in ["timeout", "connect_timeout"] {
        for within in [0, 1, 50, 99] {
            let seconds = longest - years(within);
            assert_eq!(built(seconds, member), Err(refused.to_owned()), "{within}");
        }
        for below in [101, 200] {
            assert_eq!(built(longest - years(below), member), Ok(()), "{below}");
        }
    }
}

mod certificates;
mod idle;
mod proxies;
