//! Renewable credentials are resolved for each request and errors retain the
//! exact credential used by that response, never a shared mutable token.
//!
//! Source: `tests/agent/credentials.rs`, each test under its name there.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;

use super::room::{Room, streamed_saying, with_header};
use super::support::events;
use crate::{OpenAiConfig, TokenError, TokenFuture, TokenSource};

struct Rotating(AtomicUsize);

impl TokenSource for Rotating {
    fn token(&self) -> TokenFuture<'_> {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(format!("rotating-{n}")) })
    }
}

struct Refused;

impl TokenSource for Refused {
    fn token(&self) -> TokenFuture<'_> {
        Box::pin(async { Err(TokenError::SignInRequired) })
    }
}

/// The `Authorization` of each request `room` sent, in order.
fn authorizations(room: &Room) -> Vec<Option<String>> {
    let requests = room.requests().into_iter();
    requests.map(|request| request.authorization).collect()
}

#[tokio::test]
async fn a_rotating_source_is_resolved_again_and_its_current_error_is_redacted() {
    let source = Arc::new(Rotating(AtomicUsize::new(0)));
    let room = Room::configured(
        vec![
            streamed_saying("First"),
            events(&[json!({"error":{"message":"credential rotating-1 refused"}})]),
        ],
        "unused-static",
        |config| {
            let shared = Arc::clone(&source);
            OpenAiConfig {
                token_source: Some(shared),
                ..config
            }
        },
    )
    .await;
    assert_eq!(room.says("First").await.error, None);
    let failed = room.says("Second").await;
    let error = failed.error.unwrap().message;
    assert!(!error.contains("rotating-1"), "{error}");
    let sent = ["Bearer rotating-0", "Bearer rotating-1"].map(|sent| Some(sent.to_owned()));
    assert_eq!(authorizations(&room), sent);
    assert_eq!(source.0.load(Ordering::SeqCst), 2);
}

/// A request tried again under `max_retries` asks its source again, as
/// Python's SDK refreshes a key before each try.
#[tokio::test]
async fn each_try_of_a_request_resolves_its_credential() {
    let source = Arc::new(Rotating(AtomicUsize::new(0)));
    let room = Room::configured(
        vec![
            with_header(503, "{}", "retry-after-ms", "1"),
            streamed_saying("Recovered"),
        ],
        "unused-static",
        |config| {
            let shared = Arc::clone(&source);
            OpenAiConfig {
                token_source: Some(shared),
                max_retries: 1,
                ..config
            }
        },
    )
    .await;
    assert_eq!(room.says("Hello").await.error, None);
    let sent = ["Bearer rotating-0", "Bearer rotating-1"].map(|sent| Some(sent.to_owned()));
    assert_eq!(authorizations(&room), sent);
}

#[tokio::test]
async fn a_failed_credential_source_never_falls_back_to_the_static_key() {
    let room = Room::configured(vec![], "unused-static", |config| OpenAiConfig {
        token_source: Some(Arc::new(Refused)),
        ..config
    })
    .await;
    let result = room.says("Hello").await;
    assert!(result.error.unwrap().message.contains("sign in again"));
    assert!(room.requests().is_empty());
}

// An `Authorization` the host's headers carry replaces the key's bearer,
// and a token's alike: the source is then not asked.
#[tokio::test]
async fn a_configured_authorization_replaces_the_token_of_a_source() {
    let source = Arc::new(Rotating(AtomicUsize::new(0)));
    let room = Room::configured(vec![streamed_saying("First")], "unused-static", |config| {
        let shared = Arc::clone(&source);
        let header = ("Authorization".to_owned(), "Custom host-made".to_owned());
        OpenAiConfig {
            token_source: Some(shared),
            default_headers: Some(BTreeMap::from([header])),
            ..config
        }
    })
    .await;
    assert_eq!(room.says("First").await.error, None);
    assert_eq!(authorizations(&room), [Some("Custom host-made".to_owned())]);
    assert_eq!(source.0.load(Ordering::SeqCst), 0);
}
