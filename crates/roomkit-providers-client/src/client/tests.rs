//! The timeouts a client is built with, and the exit it makes.

use std::time::{Duration, Instant};

use super::Timeouts;
use crate::Failure;
use crate::failure::{MESSAGE_LIMIT, told};
use crate::testing::{client, left, usual};

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

// The exit a client makes repeats nothing of what its provider gave it: a
// key, the value of a configured header and the credentials it holds after
// a scheme, the user information of the URL, as written and as it reads,
// each part and the two together, and the Basic credentials the client
// writes of it. A text shorter than a secret is left, and so is a header's
// scheme.
#[test]
fn the_exit_a_client_makes_repeats_nothing_of_a_configuration() {
    let url = "https://us%65r-name:pass%20word-long@llm.example/v1";
    let url = reqwest::Url::parse(url).unwrap();
    let keys = ["placeholder-not-a-key".to_owned()];
    let headers = [
        "Bearer placeholder-of-a-gateway",
        "placeholder-tenant",
        "blue",
    ];
    let exit = client(usual(), 0).exit("test", keys, headers, &url);
    let repeated = |said: &str| {
        let said = vec![told(said, MESSAGE_LIMIT)];
        exit.leave(&Failure::said(said, false, None)).message
    };
    for (said, cleaned) in [
        ("the key placeholder-not-a-key", "the key [***]"),
        (
            "Bearer placeholder-of-a-gateway, or placeholder-of-a-gateway alone",
            "[***], or [***] alone",
        ),
        (
            "a Bearer of placeholder-tenant, blue",
            "a Bearer of [***], blue",
        ),
        ("us%65r-name and user-name", "[***] and [***]"),
        ("pass%20word-long and pass word-long", "[***] and [***]"),
        (
            "us%65r-name:pass%20word-long and user-name:pass word-long",
            "[***] and [***]",
        ),
        (
            "Basic dXNlci1uYW1lOnBhc3Mgd29yZC1sb25n, or dXNlci1uYW1lOnBhc3Mgd29yZC1sb25n",
            "[***], or [***]",
        ),
    ] {
        assert_eq!(repeated(said), cleaned);
    }
    assert_eq!(exit.leave(&Failure::no_event()).provider, "test");
    // A URL that holds no user information brings no secret.
    let bare = reqwest::Url::parse("https://llm.example/v1").unwrap();
    let exit = client(usual(), 0).exit("test", [], [""; 0], &bare);
    assert_eq!(
        format!("{exit:?}"),
        r#"Exit { provider: "test", secrets: Secrets(0) }"#
    );
}

mod certificates;
mod idle;
mod proxies;
#[cfg(feature = "replay")]
mod replay;
