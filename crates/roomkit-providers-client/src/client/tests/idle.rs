//! The connections a client keeps: for how long a request takes again one
//! that no request uses, when the client closes it, and how many it keeps
//! for a host. The time and the count are written here again, not read
//! from the client: the tests hold it to them.
//!
//! No test waits: the time is let pass at once, and a test waits only on
//! what the server saw.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use reqwest::header::HeaderMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Barrier, watch};
use tokio::task::JoinHandle;

use crate::harness::let_pass;
use crate::testing::{client, usual};
use crate::{Call, Client};

/// For how long a request takes again a connection no request uses.
const KEPT_FOR: Duration = Duration::from_secs(5);

/// How many connections no request uses the client keeps for a host.
const KEPT_AT_MOST: usize = 100;

/// A local server that keeps its connections, as an endpoint that keeps a
/// connection alive does: it answers each request with an empty JSON
/// document, then waits for the next on the same connection. The harness's
/// fake closes each connection after one answer, so it cannot show which
/// connection a request came on.
struct Keeping {
    url: reqwest::Url,
    /// The connections opened so far.
    opened: Arc<AtomicUsize>,
    /// The connections its client closed so far.
    closed: watch::Receiver<usize>,
    server: JoinHandle<()>,
}

impl Keeping {
    /// A server that answers its requests `together`: none before that many
    /// wait for their answer, so that each of them came on a connection of
    /// its own.
    async fn serving(together: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/models", listener.local_addr().unwrap());
        let opened = Arc::new(AtomicUsize::new(0));
        let (closing, closed) = watch::channel(0);
        let (counted, waiting) = (Arc::clone(&opened), Arc::new(Barrier::new(together)));
        let server = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                counted.fetch_add(1, Ordering::Relaxed);
                tokio::spawn(kept(socket, Arc::clone(&waiting), closing.clone()));
            }
        });
        Self {
            url: reqwest::Url::parse(&url).unwrap(),
            opened,
            closed,
            server,
        }
    }

    /// Asks the server `requests` times at once through `client`, then lets
    /// each connection go back among those the client keeps.
    async fn asked(&self, client: &Client, requests: usize) {
        let asking = (0..requests).map(|_| {
            let (client, url) = (client.clone(), self.url.clone());
            tokio::spawn(async move {
                let headers = HeaderMap::new();
                let call = Call {
                    url: &url,
                    headers: &headers,
                    body: None,
                    token: None,
                };
                client.send(call).await.map(|_| ())
            })
        });
        for asked in asking.collect::<Vec<_>>() {
            asked.await.unwrap().unwrap();
        }
        // A connection goes back once its answer was read to its end, which
        // the client's own tasks see: they run before this one goes on.
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
    }

    /// The connections opened so far.
    fn opened(&self) -> usize {
        self.opened.load(Ordering::Relaxed)
    }

    /// Returns once the client closed `connections` connections or more.
    async fn closed(&self, connections: usize) {
        let mut closed = self.closed.clone();
        let reached = closed.wait_for(|closed| *closed >= connections);
        let reached = tokio::time::timeout(Duration::from_secs(30), reached).await;
        reached.expect("the client keeps the connection").unwrap();
    }
}

impl Drop for Keeping {
    fn drop(&mut self) {
        self.server.abort();
    }
}

/// Serves `socket` to its end: each request, which has no body, is answered
/// once `waiting` lets it, and `closing` counts the connection once its
/// client closed it.
async fn kept(mut socket: TcpStream, waiting: Arc<Barrier>, closing: watch::Sender<usize>) {
    const ANSWER: &[u8] =
        b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\n\r\n{}";
    let (mut received, mut buffer) = (Vec::new(), [0_u8; 1024]);
    loop {
        while !received.ends_with(b"\r\n\r\n") {
            match socket.read(&mut buffer).await {
                Ok(0) | Err(_) => return closing.send_modify(|closed| *closed += 1),
                Ok(read) => received.extend_from_slice(&buffer[..read]),
            }
        }
        received.clear();
        waiting.wait().await;
        if socket.write_all(ANSWER).await.is_err() {
            return closing.send_modify(|closed| *closed += 1);
        }
    }
}

// A request takes again a connection no request used for less than 5
// seconds, and none one that waited longer: the next request opens another,
// and the first is closed. Left with no request at all, a connection is
// closed by the client itself, which looks at those it keeps every 5
// seconds: 10 seconds after its last request at the latest.
#[tokio::test]
async fn an_idle_connection_is_taken_again_for_5_seconds_and_closed_within_10() {
    let server = Keeping::serving(1).await;
    let client = client(usual(), 0);
    server.asked(&client, 1).await;
    assert_eq!(server.opened(), 1);
    let_pass(KEPT_FOR.saturating_sub(Duration::from_millis(500))).await;
    server.asked(&client, 1).await;
    assert_eq!(server.opened(), 1, "taken again before its time passed");
    let_pass(KEPT_FOR + Duration::from_millis(1)).await;
    server.asked(&client, 1).await;
    assert_eq!(server.opened(), 2, "not taken again once its time passed");
    server.closed(1).await;
    let_pass(2 * KEPT_FOR + Duration::from_millis(1)).await;
    server.closed(2).await;
    assert_eq!(server.opened(), 2);
}

// Of 101 connections to one host that no request uses any more, the client
// keeps 100 and closes one: 101 requests at once then go on the 100 kept
// and on one new connection.
#[tokio::test]
async fn no_more_than_100_idle_connections_are_kept_for_a_host() {
    let at_once = KEPT_AT_MOST + 1;
    let server = Keeping::serving(at_once).await;
    let client = client(usual(), 0);
    server.asked(&client, at_once).await;
    assert_eq!(server.opened(), at_once);
    server.asked(&client, at_once).await;
    assert_eq!(server.opened(), at_once + 1);
    // One connection past the 100 was closed each time.
    server.closed(2).await;
    assert_eq!(*server.closed.borrow(), 2);
}
