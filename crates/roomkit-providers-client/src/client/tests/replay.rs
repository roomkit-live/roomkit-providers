//! The replay seam: a client a replay drives outside a provider crate's
//! tests.

use std::time::Duration;

use reqwest::header::HeaderMap;

use crate::Call;
use crate::client::replayed_at;
use crate::harness::fake::{Body, Fake, Reply};
use crate::testing::{client, left, url_on, usual};

// A replayed client reads a `retry-after` date against the clock it is
// given, and keeps the wait instead of taking it. A replay is sent to a
// loopback address, and to no other.
#[tokio::test]
async fn a_replayed_client_reads_a_date_against_its_clock_and_keeps_its_waits() {
    // The first instant of 2026, in seconds since 1970, and a date 100
    // seconds later.
    const CLOCK: f64 = 1_767_225_600.0;
    const LATER: &str = "Thu, 01 Jan 2026 00:01:40 GMT";
    let asking = Reply::Answer {
        status: 429,
        headers: vec![("retry-after".to_owned(), LATER.to_owned())],
        body: Body::Whole(Vec::new()),
    };
    let fake = Fake::serving(vec![asking, Reply::json(200, "{}")]).await;
    let mut client = client(usual(), 1);
    let waits = client.replaying(CLOCK);
    let (url, headers) = (url_on(&fake, "/v1/answers"), HeaderMap::new());
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
    };
    // A wait that was taken would outlive this.
    let answered = tokio::time::timeout(Duration::from_secs(20), client.send(call));
    answered.await.expect("no wait is taken").unwrap();
    assert_eq!(waits.taken(), [100.0]);

    let mut moved = url.clone();
    let off_the_machine = "192.0.2.1:80".parse().unwrap();
    let refused = replayed_at([&mut moved], off_the_machine).unwrap_err();
    assert_eq!(
        left(&refused).message,
        "the provider cannot be built: a replay sends to a loopback address only"
    );
    assert_eq!(moved, url);
}
