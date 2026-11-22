//! Asks a server that speaks Anthropic's Messages API once whole and once
//! streamed, and checks what it read.
//!
//! The server is a stand-in this example starts on a local port: nothing
//! leaves the machine, and the key is a placeholder. Against a real server,
//! only the configuration changes: its base URL, its key and its model.
//!
//! ```text
//! cargo run -p roomkit-providers-anthropic --example local_messages
//! ```

use std::error::Error;
use std::fmt::Write as _;
use std::future::poll_fn;
use std::io::Write as _;

use roomkit_ai::{AiContext, AiMessage, AiProvider, AiRole, FinishReason, StreamEvent};
use roomkit_providers_anthropic::{Anthropic, AnthropicConfig};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// What the stand-in server says, in the pieces it streams it in.
const PIECES: [&str; 3] = ["We ", "open ", "at nine."];

/// The events of the message the stand-in server answers with: its start,
/// one block of text in pieces, why it stopped and what it used, its end.
fn events() -> Vec<Value> {
    let usage = json!({"input_tokens": 12, "output_tokens": 1});
    let message = json!({"id": "msg_1", "type": "message", "role": "assistant",
        "model": "local-model", "content": [], "usage": usage});
    let said = |text: &str| {
        json!({"type": "content_block_delta", "index": 0,
        "delta": {"type": "text_delta", "text": text}})
    };
    let mut events = vec![
        json!({"type": "message_start", "message": message}),
        json!({"type": "content_block_start", "index": 0,
            "content_block": {"type": "text", "text": ""}}),
    ];
    events.extend(PIECES.map(said));
    events.extend([
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": 5}}),
        json!({"type": "message_stop"}),
    ]);
    events
}

/// The message as a stream of events, as an HTTP answer that ends with its
/// connection.
fn streamed() -> String {
    let mut answer = String::from(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
    );
    for event in events() {
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        // Writing to a `String` does not fail.
        write!(answer, "event: {kind}\ndata: {event}\n\n").ok();
    }
    answer
}

/// What the stand-in server answers a request that carries no key, or that
/// asks for no stream.
fn refused() -> String {
    let body = json!({"type": "error",
        "error": {"type": "invalid_request_error", "message": "a key and a stream are asked"}})
    .to_string();
    let length = body.len();
    format!(
        "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\n\
         content-length: {length}\r\nconnection: close\r\n\r\n{body}"
    )
}

/// The request `socket` sends: its head in lower case, and its body, as
/// long as its `content-length` says.
async fn request_of(socket: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut received = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        received.extend_from_slice(buffer.get(..read)?);
        let ends_the_head = |window: &[u8]| window == b"\r\n\r\n";
        let Some(head_end) = received.windows(4).position(ends_the_head) else {
            continue;
        };
        let (head, body) = received.split_at_checked(head_end.checked_add(4)?)?;
        let head = String::from_utf8_lossy(head).to_ascii_lowercase();
        let declared = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"));
        let length: usize = declared.and_then(|length| length.trim().parse().ok())?;
        if body.len() >= length {
            return Some((head, body.to_vec()));
        }
    }
}

/// Answers one request: the message as a stream, when the request carries
/// the key and asks for a stream, as every generation does.
async fn answer(mut socket: TcpStream) {
    let Some((head, body)) = request_of(&mut socket).await else {
        return;
    };
    let asked: Value = serde_json::from_slice(&body).unwrap_or_default();
    let stream = asked.get("stream").and_then(Value::as_bool) == Some(true);
    let keyed = head.contains("x-api-key: placeholder-not-a-key\r\n");
    let reply = if stream && keyed {
        streamed()
    } else {
        refused()
    };
    // A client that left before its answer is no failure of the server.
    socket.write_all(reply.as_bytes()).await.ok();
    socket.shutdown().await.ok();
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(answer(socket));
        }
    });

    let config = AnthropicConfig {
        base_url: Some(format!("http://{address}")),
        ..AnthropicConfig::new("placeholder-not-a-key", "local-model")
    };
    let provider = Anthropic::new(config)?;
    let request = AiContext {
        system_prompt: Some("You answer in one sentence.".to_owned()),
        messages: vec![AiMessage::new(AiRole::User, "When do you open?")],
        ..AiContext::default()
    };
    let mut shown = std::io::stdout().lock();

    // Once whole: the answer is folded from the stream every call asks.
    let response = provider.generate(&request).await?;
    writeln!(shown, "whole:    {}", response.content)?;
    writeln!(shown, "usage:    {}", Value::Object(response.usage.clone()))?;

    // Once streamed: the text as it comes, then how the answer ended.
    // Dropping the stream before its end would stop the request.
    let mut pieces = Vec::new();
    let mut finish_reason = None;
    let mut events = provider.generate_structured_stream(&request);
    while let Some(event) = poll_fn(|context| events.as_mut().poll_next(context)).await {
        match event? {
            StreamEvent::TextDelta(delta) => pieces.push(delta.text),
            StreamEvent::Done(done) => finish_reason = done.finish_reason,
            StreamEvent::ThinkingDelta(_)
            | StreamEvent::ToolCallDelta(_)
            | StreamEvent::ToolCall(_)
            | StreamEvent::Usage(_) => {}
        }
    }
    drop(events);
    writeln!(shown, "streamed: {}", pieces.join("|"))?;
    server.abort();

    let said = PIECES.concat();
    let counted = json!({"input_tokens": 12, "output_tokens": 5});
    let stopped = response.finish_reason == Some(FinishReason::Stop);
    if response.content != said || Value::Object(response.usage) != counted || !stopped {
        return Err("the whole answer is not what the server said".into());
    }
    if pieces != PIECES || finish_reason != Some(FinishReason::Stop) {
        return Err("the streamed answer is not what the server said".into());
    }
    Ok(())
}
