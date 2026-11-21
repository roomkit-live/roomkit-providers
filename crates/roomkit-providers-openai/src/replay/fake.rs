//! A fake of the endpoint: a local server that answers each connection with
//! the next reply of a script, and keeps what it was sent.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
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
    /// Sent read by read, each read one chunk of the transfer, written and
    /// flushed before the next; then the stream ends, the connection is
    /// lost, or nothing more comes.
    Reads(Vec<Vec<u8>>, Ending),
    /// Sent read by read as [`Body::Reads`], each read once the test says
    /// so, with one notification; then the stream ends.
    Stepped(Vec<Vec<u8>>, Arc<Notify>),
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
    /// The headers, their names in lower case: the last of each name.
    pub(crate) headers: BTreeMap<String, String>,
    /// Every header line, in the order sent, its name in lower case.
    pub(crate) lines: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
    /// How many reads its answer was written in, each flushed before the
    /// next: none for an answer sent whole.
    pub(crate) reads: usize,
}

pub(crate) struct Fake {
    pub(crate) address: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
    /// Told what the connections live: a request received, a connection
    /// its client closed.
    told: Arc<Told>,
    /// The connections being served.
    serving: Arc<Mutex<Vec<JoinHandle<()>>>>,
    server: JoinHandle<()>,
}

#[derive(Default)]
struct Told {
    received: Notify,
    silent: Notify,
    closed: Notify,
}

/// A local port a fake listens on. Fakes that serve one after the other may
/// share one: a port a server closed its connections on stays taken for a
/// minute, and a test that opens hundreds of them leaves none to open.
#[derive(Clone)]
pub(crate) struct Door(Arc<TcpListener>);

impl Door {
    /// A port of its own on this machine.
    pub(crate) async fn open() -> Self {
        Self(Arc::new(TcpListener::bind("127.0.0.1:0").await.unwrap()))
    }
}

impl Fake {
    /// A server on a port of its own that answers with `replies`, in order;
    /// a request past them is kept and answered 599.
    pub(crate) async fn serving(replies: Vec<Reply>) -> Self {
        Self::behind(&Door::open().await, replies)
    }

    /// As [`serving`](Self::serving), on the port of `door`: for fakes that
    /// serve one after the other, each dropped before the next is made.
    pub(crate) fn behind(door: &Door, replies: Vec<Reply>) -> Self {
        let listener = Arc::clone(&door.0);
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&seen);
        let told = Arc::new(Told::default());
        let telling = Arc::clone(&told);
        let serving = Arc::new(Mutex::new(Vec::new()));
        let accepted = Arc::clone(&serving);
        let server = tokio::spawn(async move {
            let mut replies = replies.into_iter();
            while let Ok((socket, _)) = listener.accept().await {
                let extra = Reply::Answer {
                    status: 599,
                    headers: Vec::new(),
                    body: Body::Whole(Vec::new()),
                };
                let reply = replies.next().unwrap_or(extra);
                // Each read goes out as it is written.
                socket.set_nodelay(true).ok();
                let telling = Arc::clone(&telling);
                let answer = answer(socket, reply, Arc::clone(&kept), telling);
                // Kept before it can end: a connection that was answered
                // is one `served` waits for.
                let mut accepted = accepted.lock().unwrap();
                accepted.push(tokio::spawn(answer));
            }
        });
        Self {
            address,
            seen,
            told,
            serving,
            server,
        }
    }

    /// Returns once every connection accepted so far was served to its
    /// end: its answer written, and its client gone where the fake keeps
    /// the connection open. What was [`seen`](Self::seen) then says in how
    /// many reads each answer was written.
    pub(crate) async fn served(&self) {
        let serving = std::mem::take(&mut *self.serving.lock().unwrap());
        for connection in serving {
            connection.await.ok();
        }
    }

    /// Returns once a request was received.
    pub(crate) async fn received(&self) {
        self.told.received.notified().await;
    }

    /// Returns once the fake stopped answering a request and keeps its
    /// connection open and silent: once for each time it does.
    pub(crate) async fn silent(&self) {
        self.told.silent.notified().await;
    }

    /// Returns once the client closed a connection the fake kept open and
    /// silent.
    pub(crate) async fn closed(&self) {
        self.told.closed.notified().await;
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
async fn answer(mut socket: TcpStream, reply: Reply, seen: Arc<Mutex<Vec<Seen>>>, told: Arc<Told>) {
    let Some(request) = read_request(&mut socket).await else {
        return;
    };
    let at = {
        let mut seen = seen.lock().unwrap();
        seen.push(request);
        seen.len() - 1
    };
    // Each read is counted as it is written.
    let wrote = || {
        if let Some(seen) = seen.lock().unwrap().get_mut(at) {
            seen.reads += 1;
        }
    };
    told.received.notify_one();
    let closed = &*told;
    let (status, headers, body) = match reply {
        Reply::Refused => return,
        Reply::Silent => return hold(socket, closed).await,
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
            send_reads(&mut socket, head, &reads, None, &wrote).await;
            match ending {
                Ending::Ends => end_reads(socket).await,
                Ending::Lost => {}
                Ending::Silent => hold(socket, closed).await,
            }
        }
        Body::Stepped(reads, steps) => {
            send_reads(&mut socket, head, &reads, Some(&steps), &wrote).await;
            end_reads(socket).await;
        }
    }
}

/// Sends the head of a chunked answer, then each of `reads` as one chunk of
/// the transfer, flushed before the next, each once `steps` says so when
/// steps are given. `wrote` is told of each read as it is written.
async fn send_reads(
    socket: &mut TcpStream,
    mut head: Vec<String>,
    reads: &[Vec<u8>],
    steps: Option<&Notify>,
    wrote: &(dyn Fn() + Sync),
) {
    head.push("transfer-encoding: chunked\r\n\r\n".to_owned());
    socket.write_all(head.join("\r\n").as_bytes()).await.ok();
    for read in reads.iter().filter(|read| !read.is_empty()) {
        if let Some(steps) = steps {
            socket.flush().await.ok();
            steps.notified().await;
        }
        let mut chunk = format!("{:x}\r\n", read.len()).into_bytes();
        chunk.extend_from_slice(read);
        chunk.extend_from_slice(b"\r\n");
        wrote();
        socket.write_all(&chunk).await.ok();
        socket.flush().await.ok();
    }
}

/// Ends a chunked answer.
async fn end_reads(mut socket: TcpStream) {
    socket.write_all(b"0\r\n\r\n").await.ok();
    socket.shutdown().await.ok();
}

/// Keeps a connection open and silent until its client closes it, and
/// tells so.
async fn hold(mut socket: TcpStream, told: &Told) {
    told.silent.notify_one();
    let mut sent = [0_u8; 64];
    while matches!(socket.read(&mut sent).await, Ok(1..)) {}
    told.closed.notify_one();
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
        let (name, value) = (name.to_ascii_lowercase(), value.trim().to_owned());
        seen.lines.push((name.clone(), value.clone()));
        seen.headers.insert(name, value);
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
