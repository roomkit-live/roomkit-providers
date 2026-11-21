//! A fake of the endpoint: a local server that answers each connection with
//! the next reply of a script, and keeps what it was sent.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// What the fake does with one request.
#[derive(Clone, Debug)]
pub(crate) enum Reply {
    /// Reads the request and closes the connection: no answer comes.
    Refused,
    /// Reads the request and never answers.
    Silent,
    /// Answers with a status, headers and a body.
    Answer {
        status: u16,
        headers: Vec<(String, String)>,
        body: Body,
    },
}

/// The body of an answer.
#[derive(Clone, Debug)]
pub(crate) enum Body {
    /// Sent whole, its length declared.
    Whole(Vec<u8>),
    /// Sent read by read; then the stream ends, the connection is lost, or
    /// nothing more comes.
    Reads(Vec<Vec<u8>>, Ending),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ending {
    Ends,
    Lost,
    Silent,
}

/// A request the fake was sent.
#[derive(Clone, Debug, Default)]
pub(crate) struct Seen {
    pub(crate) method: String,
    /// The path and the query.
    pub(crate) target: String,
    /// The headers, their names in lower case.
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: Vec<u8>,
}

pub(crate) struct Fake {
    pub(crate) address: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
    server: JoinHandle<()>,
}

impl Fake {
    /// A server on a port of its own that answers with `replies`, in order;
    /// a request past them is kept and answered 599.
    pub(crate) async fn serving(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&seen);
        let server = tokio::spawn(async move {
            let mut replies = replies.into_iter();
            while let Ok((socket, _)) = listener.accept().await {
                let extra = Reply::Answer {
                    status: 599,
                    headers: Vec::new(),
                    body: Body::Whole(Vec::new()),
                };
                let reply = replies.next().unwrap_or(extra);
                tokio::spawn(answer(socket, reply, Arc::clone(&kept)));
            }
        });
        Self {
            address,
            seen,
            server,
        }
    }

    /// The requests received so far, in order.
    pub(crate) fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.server.abort();
    }
}

/// Reads one request from `socket`, keeps it, and answers as `reply` says.
async fn answer(mut socket: TcpStream, reply: Reply, seen: Arc<Mutex<Vec<Seen>>>) {
    let Some(request) = read_request(&mut socket).await else {
        return;
    };
    seen.lock().unwrap().push(request);
    let (status, headers, body) = match reply {
        Reply::Refused => return,
        Reply::Silent => return hold(socket).await,
        Reply::Answer {
            status,
            headers,
            body,
        } => (status, headers, body),
    };
    let mut head = vec![
        format!("HTTP/1.1 {status} Status"),
        "connection: close".to_owned(),
    ];
    head.extend(
        headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}")),
    );
    match body {
        Body::Whole(body) => {
            head.push(format!("content-length: {}\r\n\r\n", body.len()));
            socket.write_all(head.join("\r\n").as_bytes()).await.ok();
            socket.write_all(&body).await.ok();
            socket.shutdown().await.ok();
        }
        Body::Reads(reads, ending) => {
            head.push("transfer-encoding: chunked\r\n\r\n".to_owned());
            socket.write_all(head.join("\r\n").as_bytes()).await.ok();
            for read in reads.iter().filter(|read| !read.is_empty()) {
                socket
                    .write_all(format!("{:x}\r\n", read.len()).as_bytes())
                    .await
                    .ok();
                socket.write_all(read).await.ok();
                socket.write_all(b"\r\n").await.ok();
                socket.flush().await.ok();
            }
            match ending {
                Ending::Ends => {
                    socket.write_all(b"0\r\n\r\n").await.ok();
                    socket.shutdown().await.ok();
                }
                Ending::Lost => {}
                Ending::Silent => hold(socket).await,
            }
        }
    }
}

/// Keeps a connection open and silent, far past any timeout of a test.
async fn hold(socket: TcpStream) {
    tokio::time::sleep(Duration::from_secs(600)).await;
    drop(socket);
}

/// One request: its head, then the body its `content-length` declares.
async fn read_request(socket: &mut TcpStream) -> Option<Seen> {
    let mut received = Vec::new();
    let mut buffer = [0_u8; 8192];
    let head_end = loop {
        if let Some(at) = received.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break at;
        }
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        received.extend_from_slice(&buffer[..read]);
    };
    let head = String::from_utf8_lossy(&received[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let mut seen = Seen {
        method: request_line.next()?.to_owned(),
        target: request_line.next()?.to_owned(),
        ..Seen::default()
    };
    for line in lines {
        let (name, value) = line.split_once(':')?;
        seen.headers
            .insert(name.to_ascii_lowercase(), value.trim().to_owned());
    }
    let length = seen.headers.get("content-length");
    let length: usize = length.and_then(|length| length.parse().ok()).unwrap_or(0);
    let mut body = received[head_end + 4..].to_vec();
    while body.len() < length {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    seen.body = body;
    Some(seen)
}
