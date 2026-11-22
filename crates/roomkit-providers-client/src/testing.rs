//! What the crate's tests share: a client as a provider would build it, an
//! exit with no secret, and the URL of a local fake. A test's client reads
//! no environment: it is given its proxies and its TLS.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use futures_core::Stream;
use roomkit_ai::ProviderError;

use crate::failure::{Exit, Failure, Secrets, Stated};
use crate::harness::fake::Fake;
use crate::items::next;
use crate::proxy::Proxies;
use crate::retry::{Past, Rule};
use crate::{Client, Settings, Timeouts};

/// The timeouts of a test that states none: 30 seconds for a read, 5 to
/// connect.
pub(crate) fn usual() -> Timeouts {
    Timeouts::new(Duration::from_secs(30), Duration::from_secs(5)).unwrap()
}

/// What a client states that tries a failed request `max_retries` times
/// again within `timeouts`, and words a refusal from its status alone.
pub(crate) fn settings(timeouts: Timeouts, max_retries: u32) -> Settings {
    Settings {
        timeouts,
        max_retries,
        retry: Rule {
            longest_asked: 120.0,
            past_it: Past::Final,
        },
        refused: |status, body| Failure::refused(status, body, Stated::default()),
        tried_again: crate::tried_again!("roomkit::providers::test"),
    }
}

/// TLS that trusts no authority: for a test whose fake speaks plain HTTP.
pub(crate) fn no_authority() -> rustls::ClientConfig {
    let cryptography = Arc::new(rustls::crypto::ring::default_provider());
    rustls::ClientConfig::builder_with_provider(cryptography)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth()
}

/// A client with `timeouts` that tries a failed request `max_retries` times
/// again, and words a refusal from its status alone. It goes through no
/// proxy, whatever the environment of the test names.
pub(crate) fn client(timeouts: Timeouts, max_retries: u32) -> Client {
    let settings = settings(timeouts, max_retries);
    Client::built(settings, Proxies::default(), no_authority()).unwrap()
}

/// `failure` as it leaves a provider that has no secret.
pub(crate) fn left(failure: &Failure) -> ProviderError {
    Exit::new("test", Secrets::default()).leave(failure)
}

/// The URL of `path` on `fake`.
pub(crate) fn url_on(fake: &Fake, path: &str) -> reqwest::Url {
    reqwest::Url::parse(&format!("http://{}{path}", fake.address)).unwrap()
}

/// The polls after which a future still pending is taken for stuck.
const POLLS: usize = 100_000;

/// The output of `future`, polled on this thread until it is ready: for a
/// future that waits on nothing but its own reader.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    for _ in 0..POLLS {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
    panic!("a future of a test is still pending after {POLLS} polls");
}

/// Every item of `stream`, in order.
pub(crate) fn collected<S: Stream + Unpin>(mut stream: S) -> Vec<S::Item> {
    block_on(async {
        let mut items = Vec::new();
        while let Some(item) = next(&mut stream).await {
            items.push(item);
        }
        items
    })
}
