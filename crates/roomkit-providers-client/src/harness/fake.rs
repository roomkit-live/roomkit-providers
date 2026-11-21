//! A fake of the endpoint: a local server that answers each connection with
//! the next reply of a script, and keeps what it was sent. It speaks plain
//! HTTP, or TLS with the certificate a test's authority signed
//! ([`Fake::serving_tls`]); and it stands for a proxy as it stands for an
//! endpoint, since a proxy is asked as a server is: it answers a request
//! handed to it whole, and carries a tunnel ([`Reply::Tunnel`]).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

/// What the fake does with one request.
#[derive(Clone, Debug)]
pub enum Reply {
    /// Reads the request and closes the connection: no answer comes.
    Refused,
    /// Reads the request and never answers.
    Silent,
    /// Answers with a status, headers and a body.
    Answer {
        /// The status of the answer.
        status: u16,
        /// The headers of the answer, beside those the fake writes of the
        /// transfer.
        headers: Vec<(String, String)>,
        /// The body of the answer, and how it is sent.
        body: Body,
    },
    /// Answers a `CONNECT` with a 200, then carries what the client sends
    /// to the server at this address, and what that server sends back: the
    /// tunnel of a proxy, whatever host the client asked it for.
    Tunnel(SocketAddr),
}

impl Reply {
    /// An answer of HTTP `status` the server says is JSON, whose body is
    /// `body`, sent whole.
    pub fn json(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self::Answer {
            status,
            headers: vec![("content-type".to_owned(), "application/json".to_owned())],
            body: Body::Whole(body.into()),
        }
    }

    /// A streamed answer of HTTP 200 the server says is a stream of events:
    /// `reads`, each sent as one read, and how they end.
    #[must_use]
    pub fn stream(reads: Vec<Vec<u8>>, ending: Ending) -> Self {
        Self::Answer {
            status: 200,
            headers: vec![("content-type".to_owned(), "text/event-stream".to_owned())],
            body: Body::Reads(reads, ending),
        }
    }
}

/// The body of an answer.
#[derive(Clone, Debug)]
pub enum Body {
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

/// How the reads of a body end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// The stream ends.
    Ends,
    /// The connection is lost.
    Lost,
    /// Nothing more comes, and the connection stays open.
    Silent,
}

/// A request the fake was sent.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    /// The method of the request.
    pub method: String,
    /// The path and the query.
    pub target: String,
    /// The headers, their names in lower case: the last of each name.
    pub headers: BTreeMap<String, String>,
    /// Every header line, in the order sent, its name in lower case.
    pub lines: Vec<(String, String)>,
    /// The body of the request.
    pub body: Vec<u8>,
    /// How many reads its answer was written in, each flushed before the
    /// next: none for an answer sent whole.
    pub reads: usize,
}

impl Seen {
    /// What the Basic credentials of the header `name` decode to, its name
    /// in lower case: the user and the password a proxy was given in a
    /// `proxy-authorization`, around a colon. `None` for a request without
    /// the header, and for a header that holds no such credentials.
    #[must_use]
    pub fn basic(&self, name: &str) -> Option<String> {
        let encoded = self.headers.get(name)?.strip_prefix("Basic ")?;
        let mut decoded = Vec::new();
        let (mut bits, mut held) = (0_u32, 0_u32);
        for sign in encoded.bytes().filter(|sign| *sign != b'=') {
            let value = match sign {
                b'A'..=b'Z' => sign - b'A',
                b'a'..=b'z' => sign - b'a' + 26,
                b'0'..=b'9' => sign - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => return None,
            };
            // Six bits a sign: a byte is taken once eight are held.
            bits = ((bits << 6) | u32::from(value)) & 0xffff;
            held += 6;
            if held >= 8 {
                held -= 8;
                decoded.push(u8::try_from((bits >> held) & 0xff).ok()?);
            }
        }
        String::from_utf8(decoded).ok()
    }
}

/// A local server that answers each connection with the next reply of a
/// script, and keeps what it was sent. It stops when it is dropped.
pub struct Fake {
    /// Where the fake listens: a port of this machine.
    pub address: SocketAddr,
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
    turned_away: Notify,
}

/// A local port a fake listens on. Fakes that serve one after the other may
/// share one: a port a server closed its connections on stays taken for a
/// minute, and a test that opens hundreds of them leaves none to open.
#[derive(Clone, Debug)]
pub struct Door(Arc<TcpListener>);

impl std::fmt::Debug for Fake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fake")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl Door {
    /// A port of its own on this machine.
    pub async fn open() -> Self {
        Self(Arc::new(TcpListener::bind("127.0.0.1:0").await.unwrap()))
    }
}

impl Fake {
    /// A server on a port of its own that answers with `replies`, in order;
    /// a request past them is kept and answered 599.
    pub async fn serving(replies: Vec<Reply>) -> Self {
        Self::behind(&Door::open().await, replies)
    }

    /// As [`serving`](Self::serving), over TLS: each connection is greeted
    /// as `tls` says, the configuration of a server
    /// ([`Authority::serving`](super::tls::Authority::serving)), before
    /// its request is read. A connection its client ends before the
    /// greeting is done takes no reply of the script.
    pub async fn serving_tls(replies: Vec<Reply>, tls: Arc<rustls::ServerConfig>) -> Self {
        Self::started(&Door::open().await, replies, Some(TlsAcceptor::from(tls)))
    }

    /// As [`serving`](Self::serving), on the port of `door`: for fakes that
    /// serve one after the other, each dropped before the next is made.
    #[must_use]
    pub fn behind(door: &Door, replies: Vec<Reply>) -> Self {
        Self::started(door, replies, None)
    }

    fn started(door: &Door, replies: Vec<Reply>, tls: Option<TlsAcceptor>) -> Self {
        let listener = Arc::clone(&door.0);
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&seen);
        let told = Arc::new(Told::default());
        let telling = Arc::clone(&told);
        let serving = Arc::new(Mutex::new(Vec::new()));
        let accepted = Arc::clone(&serving);
        let server = tokio::spawn(async move {
            let replies = Arc::new(Mutex::new(replies.into_iter()));
            while let Ok((socket, _)) = listener.accept().await {
                // Each read goes out as it is written.
                socket.set_nodelay(true).ok();
                let telling = Arc::clone(&telling);
                let kept = Arc::clone(&kept);
                let answer: Answering = match tls.clone() {
                    // The reply is the next when the connection is taken.
                    None => Box::pin(answer(socket, next_of(&replies), kept, telling)),
                    Some(tls) => {
                        Box::pin(greeted(tls, socket, Arc::clone(&replies), kept, telling))
                    }
                };
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
    pub async fn served(&self) {
        let serving = std::mem::take(&mut *self.serving.lock().unwrap());
        for connection in serving {
            connection.await.ok();
        }
    }

    /// Returns once a request was received.
    pub async fn received(&self) {
        self.told.received.notified().await;
    }

    /// Returns once the fake stopped answering a request and keeps its
    /// connection open and silent: once for each time it does.
    pub async fn silent(&self) {
        self.told.silent.notified().await;
    }

    /// Returns once the client closed a connection the fake kept open and
    /// silent.
    pub async fn closed(&self) {
        self.told.closed.notified().await;
    }

    /// Returns once a client ended a connection before the TLS greeting
    /// was done, as one that refuses the fake's certificate does: once for
    /// each time one does.
    pub async fn turned_away(&self) {
        self.told.turned_away.notified().await;
    }

    /// The requests received so far, in order.
    #[must_use]
    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.server.abort();
    }
}

/// What a connection is served with, to its end.
type Answering = std::pin::Pin<Box<dyn Future<Output = ()> + Send>>;

/// The replies of a script that are left.
type Replies = Arc<Mutex<std::vec::IntoIter<Reply>>>;

/// A connection a fake may serve: a socket, or TLS over one.
trait Connection: AsyncRead + AsyncWrite + Unpin + Send {}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> Connection for S {}

/// The next reply of a script: a request past them is answered 599.
fn next_of(replies: &Replies) -> Reply {
    let extra = Reply::Answer {
        status: 599,
        headers: Vec::new(),
        body: Body::Whole(Vec::new()),
    };
    replies.lock().unwrap().next().unwrap_or(extra)
}

/// Greets `socket` over TLS, then answers it with the next reply. A client
/// that leaves before the greeting is done is told of, and takes no reply.
async fn greeted(
    tls: TlsAcceptor,
    socket: TcpStream,
    replies: Replies,
    seen: Arc<Mutex<Vec<Seen>>>,
    told: Arc<Told>,
) {
    match tls.accept(socket).await {
        Ok(socket) => answer(socket, next_of(&replies), seen, told).await,
        Err(_) => told.turned_away.notify_one(),
    }
}

/// Reads one request from `socket`, keeps it, and answers as `reply` says.
async fn answer(
    mut socket: impl Connection,
    reply: Reply,
    seen: Arc<Mutex<Vec<Seen>>>,
    told: Arc<Told>,
) {
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
        Reply::Tunnel(to) => return carry(socket, to).await,
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
    socket: &mut impl Connection,
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

/// Answers a `CONNECT` with a 200, then carries the bytes between `socket`
/// and the server at `to` until one of them ends.
async fn carry(mut socket: impl Connection, to: SocketAddr) {
    let Ok(mut server) = TcpStream::connect(to).await else {
        return;
    };
    let opened = b"HTTP/1.1 200 Connection established\r\n\r\n";
    if socket.write_all(opened).await.is_ok() {
        tokio::io::copy_bidirectional(&mut socket, &mut server)
            .await
            .ok();
    }
}

/// Ends a chunked answer.
async fn end_reads(mut socket: impl Connection) {
    socket.write_all(b"0\r\n\r\n").await.ok();
    socket.shutdown().await.ok();
}

/// Keeps a connection open and silent until its client closes it, and
/// tells so.
async fn hold(mut socket: impl Connection, told: &Told) {
    told.silent.notify_one();
    let mut sent = [0_u8; 64];
    while matches!(socket.read(&mut sent).await, Ok(1..)) {}
    told.closed.notify_one();
}

/// One request: its head, then the body its `content-length` declares.
async fn read_request(socket: &mut impl Connection) -> Option<Seen> {
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
