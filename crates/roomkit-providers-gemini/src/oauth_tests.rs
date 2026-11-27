//! OAuth on the Developer API uses the quota project and a token per HTTP exchange.
use super::*;
use roomkit_ai::{AiContext, AiMessage, AiProvider, AiRole};
use roomkit_providers_client::harness::fake::{Fake, Reply};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
struct Tokens(AtomicUsize);
impl TokenSource for Tokens {
    fn token(&self) -> TokenFuture<'_> {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(format!("fixture-{n}")) })
    }
}

#[tokio::test]
async fn oauth_is_resolved_for_each_page_and_generation() {
    let fake = Fake::serving(vec![
        Reply::json(200, r#"{"models":[],"nextPageToken":"next"}"#),
        Reply::json(200, r#"{"models":[]}"#),
        Reply::json(403, r#"{"error":{"message":"denied"}}"#),
    ])
    .await;
    let mut provider = Gemini::with_oauth(
        GeminiConfig::new(""),
        "example-project",
        Arc::new(Tokens::default()),
    )
    .unwrap();
    provider.point_at(fake.address);
    provider.list_models().await.unwrap();
    let request = AiContext {
        messages: vec![AiMessage::new(AiRole::User, "Hello")],
        ..AiContext::default()
    };
    assert!(provider.generate(&request).await.is_err());
    let seen = fake.seen();
    assert_eq!(seen.len(), 3);
    for (n, r) in seen.iter().enumerate() {
        assert_eq!(r.headers["authorization"], format!("Bearer fixture-{n}"));
        assert_eq!(r.headers["x-goog-user-project"], "example-project");
        assert!(!r.headers.contains_key("x-goog-api-key"));
    }
}
