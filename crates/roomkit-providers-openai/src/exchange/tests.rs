//! The times a request is given, and what a request that never left reads
//! as. No test here waits on a timeout: the time is let pass at once, after
//! what must have happened first is seen to have happened.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use roomkit_providers_llm::AiProvider;

use super::Pace;
use crate::ported::support::{KEY, elsewhere, hello, pointed};
use crate::replay::fake::{Fake, Reply};
use crate::replay::{let_pass, poll_once};
use crate::{OpenAi, OpenAiConfig};

// A request that gets no answer fails once the configuration's timeout
// passed, and not before: half of it later the request still waits, and
// once it passed the failure comes long before another half.
#[tokio::test]
async fn a_request_that_gets_no_answer_fails_at_the_read_timeout() {
    const TIMEOUT: Duration = Duration::from_secs(40);
    let fake = Fake::serving(vec![Reply::Silent]).await;
    let config = OpenAiConfig {
        timeout: TIMEOUT,
        ..elsewhere("local-model")
    };
    let provider = pointed(config, &fake);
    let request = hello();
    let mut asked = std::pin::pin!(provider.generate(&request));
    tokio::select! {
        biased;
        answered = &mut asked => panic!("no answer comes: {answered:?}"),
        () = fake.silent() => {}
    }
    let_pass(TIMEOUT / 2).await;
    assert!(poll_once(&mut asked).await.is_pending());
    let_pass(TIMEOUT / 2 + Duration::from_secs(1)).await;
    let failed = tokio::select! {
        biased;
        failed = &mut asked => failed,
        () = tokio::time::sleep(TIMEOUT / 2) => panic!("the request outlives its timeout"),
    };
    let error = failed.unwrap_err();
    assert_eq!(
        (error.message.as_str(), error.retryable, error.status_code),
        ("the provider did not answer in time", true, None)
    );
}

// A connection that does not open fails at the configuration's connect
// timeout, long before the timeout of a read. The address is a local one
// whose listener accepts nothing and whose queue of connections is full,
// where Linux leaves a new connection waiting.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_connection_that_does_not_open_fails_at_the_connect_timeout() {
    const CONNECT: Duration = Duration::from_secs(5);
    const READ: Duration = Duration::from_secs(600);
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.bind(([127, 0, 0, 1], 0).into()).unwrap();
    let listener = socket.listen(0).unwrap();
    let address = listener.local_addr().unwrap();
    let connect = || tokio::spawn(tokio::net::TcpStream::connect(address));
    let filling: Vec<_> = (0..4).map(|_| connect()).collect();
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
    let config = OpenAiConfig {
        base_url: Some(format!("http://{address}/v1")),
        connect_timeout: CONNECT,
        timeout: READ,
        ..OpenAiConfig::new(KEY, "local-model")
    };
    let provider = OpenAi::new(config).unwrap();
    let request = hello();
    let mut asked = std::pin::pin!(provider.generate(&request));
    // The connection begins, and waits.
    for _ in 0..16 {
        assert!(poll_once(&mut asked).await.is_pending());
        tokio::task::yield_now().await;
    }
    let_pass(CONNECT + Duration::from_secs(1)).await;
    let failed = tokio::select! {
        biased;
        failed = &mut asked => failed,
        () = tokio::time::sleep(READ / 20) => panic!("the connection outlives its timeout"),
    };
    let error = failed.unwrap_err();
    assert_eq!(
        (error.message.as_str(), error.retryable, error.status_code),
        ("the provider did not answer in time", true, None)
    );
    drop((listener, filling));
}

// A request that cannot be built never left: it fails the same however
// many times it is tried, so it is final, and no wait is taken for it.
#[tokio::test]
async fn a_request_that_cannot_be_built_is_final() {
    // A URL longer than a request line holds.
    let config = OpenAiConfig {
        base_url: Some(format!("http://127.0.0.1:9/{}", "a".repeat(70_000))),
        max_retries: 3,
        ..OpenAiConfig::new(KEY, "local-model")
    };
    let mut provider = OpenAi::new(config).unwrap();
    let waits = Arc::new(Mutex::new(0));
    let kept = Arc::clone(&waits);
    provider.pace = Pace {
        now: Arc::new(|| 0.0),
        wait: Arc::new(move |_| {
            *kept.lock().unwrap() += 1;
            Box::pin(std::future::ready(()))
        }),
    };
    let error = provider.generate(&hello()).await.unwrap_err();
    assert_eq!(
        (
            error.message.as_str(),
            error.retryable,
            error.before_request
        ),
        ("the request could not be built", false, true)
    );
    assert_eq!(*waits.lock().unwrap(), 0);
    let listed = provider.list_models().await.unwrap_err();
    assert_eq!((listed.retryable, listed.before_request), (false, true));
}
