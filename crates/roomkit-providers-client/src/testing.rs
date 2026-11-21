//! What the crate's tests share: a client as a provider would build it, an
//! exit with no secret, and the URL of a local fake.

use std::time::Duration;

use roomkit_providers_llm::ProviderError;

use crate::failure::{Exit, Failure, Secrets, Stated};
use crate::harness::fake::Fake;
use crate::retry::{Past, Rule};
use crate::{Client, Settings, Timeouts};

/// The timeouts of a test that states none: 30 seconds for a read, 5 to
/// connect.
pub(crate) fn usual() -> Timeouts {
    Timeouts::new(Duration::from_secs(30), Duration::from_secs(5)).unwrap()
}

/// A client with `timeouts` that tries a failed request `max_retries` times
/// again, and words a refusal from its status alone.
pub(crate) fn client(timeouts: Timeouts, max_retries: u32) -> Client {
    let settings = Settings {
        timeouts,
        max_retries,
        retry: Rule {
            longest_asked: 120.0,
            past_it: Past::Final,
        },
        refused: |status, body| Failure::refused(status, body, Stated::default()),
        tried_again: crate::tried_again!("roomkit::providers::test"),
    };
    Client::new(settings).unwrap()
}

/// `failure` as it leaves a provider that has no secret.
pub(crate) fn left(failure: &Failure) -> ProviderError {
    Exit::new("test", Secrets::default()).leave(failure)
}

/// The URL of `path` on `fake`.
pub(crate) fn url_on(fake: &Fake, path: &str) -> reqwest::Url {
    reqwest::Url::parse(&format!("http://{}{path}", fake.address)).unwrap()
}
