//! A room in one process, its agent in another, on one Redis server.
//!
//! The process this example starts is the room's. It starts itself again as
//! a second process, the agent's, which serves a turn executor on the same
//! server and the same key prefix. The room's process then takes a
//! person's message: the turn of the room's agent is submitted through
//! Redis, runs in the other process, and its answer comes back into the
//! room. The example checks that answer and fails otherwise.
//!
//! It needs a Redis server, which `REDIS_URL` names:
//!
//! ```sh
//! docker run --rm -p 127.0.0.1:6379:6379 redis:7
//! REDIS_URL=redis://127.0.0.1:6379 \
//!     cargo run -p roomkit-providers-redis --example two_processes
//! ```
#![expect(clippy::print_stdout, reason = "an example prints what it ran")]

use std::collections::BTreeSet;
use std::env;
use std::error::Error;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use roomkit::model::{
    ChannelCategory, ChannelId, ChannelType, EventContent, EventType, InboundMessage, RoomId,
    TextContent,
};
use roomkit::store::ListEventsOptions;
use roomkit::testing::{
    EchoAgent, EchoAgentOptions, RecordingTransport, RecordingTransportOptions,
};
use roomkit::turns::TurnBackendHandle;
use roomkit::{
    AgentIdentity, AttachChannelOptions, ChannelDeclaration, ConversationStore, CreateRoomOptions,
    ProcessInboundOptions, RoomKit, RoomKitOptions, TurnExecutor, TurnExecutorOptions,
};
use roomkit_providers_redis::{RedisTurnBackend, RedisTurnBackendOptions};

type Outcome<T = ()> = Result<T, Box<dyn Error>>;

/// What the agent answers, which the room's process checks.
const ANSWER: &str = "Hello from the other process.";

/// A process of the example's deployment: both name the same server and
/// the same key prefix, so they reach each other.
fn backend(url: &str, prefix: &str) -> Outcome<RedisTurnBackend> {
    Ok(RedisTurnBackend::new(
        Some(url),
        RedisTurnBackendOptions {
            key_prefix: prefix.to_owned(),
            ..RedisTurnBackendOptions::default()
        },
    )?)
}

/// The agent's process: it serves an executor that hosts the agent, says it
/// is ready, and serves until its standard input closes.
async fn agents_process(url: &str, prefix: &str) -> Outcome {
    let backend = backend(url, prefix)?;
    let executor = TurnExecutor::new(
        |id| async move {
            Ok(Some(EchoAgent::new(
                id,
                ANSWER,
                EchoAgentOptions::default(),
            )))
        },
        TurnExecutorOptions {
            executor_id: Some("agents-process".into()),
            hosts: Some(BTreeSet::from(["agent".into()])),
            ..TurnExecutorOptions::default()
        },
    )?;
    backend.serve(executor.clone()).await?;
    println!("ready");
    io::stdout().flush()?;
    tokio::task::spawn_blocking(|| io::stdin().lock().lines().next()).await?;
    backend.close().await?;
    executor.close().await;
    Ok(())
}

/// Starts the agent's process and waits until it serves.
fn start_agents_process(url: &str, prefix: &str) -> Outcome<Child> {
    let mut child = Command::new(env::current_exe()?)
        .args(["agent", url, prefix])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let output = child
        .stdout
        .take()
        .ok_or("the agent's process has no output")?;
    let mut ready = String::new();
    BufReader::new(output).read_line(&mut ready)?;
    if ready.trim_end() != "ready" {
        return Err(format!("the agent's process said {ready:?}, not ready").into());
    }
    Ok(child)
}

/// The room's process: a room with a person's transport and the agent,
/// which this process declares and does not host.
async fn rooms_process(url: &str, prefix: &str) -> Outcome<Vec<String>> {
    let kit = RoomKit::new(RoomKitOptions {
        turn_backend: Some(TurnBackendHandle::new(Arc::new(backend(url, prefix)?))),
        ..RoomKitOptions::default()
    });
    let (room, text, agent): (RoomId, ChannelId, ChannelId) =
        ("lobby".parse()?, "text".parse()?, "agent".parse()?);
    kit.register_channel(RecordingTransport::new(
        text.clone(),
        ChannelType::SMS,
        RecordingTransportOptions::default(),
    ))?;
    kit.register_channel(ChannelDeclaration {
        identity: Some(AgentIdentity::default()),
        ..ChannelDeclaration::new(agent.clone())
    })?;
    kit.create_room(CreateRoomOptions {
        room_id: Some(room.clone()),
        ..CreateRoomOptions::default()
    })
    .await?;
    kit.attach_channel(&room, &text, AttachChannelOptions::default())
        .await?;
    let intelligence = AttachChannelOptions {
        category: Some(ChannelCategory::Intelligence),
        ..AttachChannelOptions::default()
    };
    kit.attach_channel(&room, &agent, intelligence).await?;

    let said = InboundMessage::new(text, "person", TextContent::new("Hello?"));
    let options = ProcessInboundOptions {
        room_id: Some(room.clone()),
        ..ProcessInboundOptions::default()
    };
    let result = kit.process_inbound(said, options).await?;
    if let Some(error) = result.error {
        return Err(format!("the room refused the message: {error:?}").into());
    }
    let answers = kit
        .store()
        .list_events(&room, ListEventsOptions::default())
        .await?
        .iter()
        .filter(|row| row.source.channel_id == agent && row.r#type == EventType::Message)
        .filter_map(|row| {
            if let EventContent::Text(text) = &row.content {
                Some(text.body.clone())
            } else {
                None
            }
        })
        .collect();
    kit.close().await?;
    Ok(answers)
}

async fn run() -> Outcome {
    let arguments: Vec<String> = env::args().skip(1).collect();
    if let [role, url, prefix] = arguments.as_slice()
        && role == "agent"
    {
        return agents_process(url, prefix).await;
    }
    let Ok(url) = env::var("REDIS_URL") else {
        return Err("REDIS_URL names no Redis server: see the example's first lines".into());
    };
    // A prefix of this run's own: a server other runs share keeps them apart.
    let prefix = format!("roomkit:example:two-processes:{}", std::process::id());
    let mut agents = start_agents_process(&url, &prefix)?;
    println!("the agent's process serves its executor on Redis");

    let answers = rooms_process(&url, &prefix).await;
    // Closing its input stops the agent's process, whatever the room read.
    drop(agents.stdin.take());
    let stopped = agents.wait()?;
    let answers = answers?;
    println!("the room's process read the agent's answer: {answers:?}");
    if answers != [ANSWER] {
        return Err(format!("expected the one answer {ANSWER:?}, got {answers:?}").into());
    }
    if !stopped.success() {
        return Err(format!("the agent's process ended with {stopped}").into());
    }
    println!("the turn ran in the other process and its answer came back");
    Ok(())
}

fn main() -> Outcome {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(run())
}
