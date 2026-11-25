//! A room's turn across two processes on one Redis server: the room's
//! process submits it, the process that serves the agent's executor runs
//! it, and the registry tells every process who holds the room.
use std::collections::BTreeSet;
use std::sync::Arc;

use roomkit::model::{
    ChannelCategory, ChannelId, ChannelType, EventContent, EventType, InboundMessage, RoomId,
    TextContent,
};
use roomkit::store::ListEventsOptions;
use roomkit::testing::{
    EchoAgent, EchoAgentOptions, RecordingTransport, RecordingTransportOptions,
};
use roomkit::turns::{TurnBackendHandle, TurnRegistry};
use roomkit::{
    AgentIdentity, AttachChannelOptions, ChannelDeclaration, ConversationStore, CreateRoomOptions,
    DetachChannelOptions, ProcessInboundOptions, RoomKit, RoomKitOptions, TurnExecutor,
    TurnExecutorOptions,
};

use super::support;

/// A worker that hosts the agent and answers its own name.
fn worker(name: &'static str) -> TurnExecutor {
    TurnExecutor::new(
        move |id| async move { Ok(Some(EchoAgent::new(id, name, EchoAgentOptions::default()))) },
        TurnExecutorOptions {
            executor_id: Some(name.into()),
            hosts: Some(BTreeSet::from(["agent".into()])),
            ..TurnExecutorOptions::default()
        },
    )
    .unwrap()
}

/// The texts the agent wrote in the room once a person said hello.
async fn replies(kit: &RoomKit, room: &RoomId, text: &ChannelId, agent: &ChannelId) -> Vec<String> {
    let result = kit
        .process_inbound(
            InboundMessage::new(text.clone(), "person", TextContent::new("Hi")),
            ProcessInboundOptions {
                room_id: Some(room.clone()),
                ..ProcessInboundOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(result.error, None);
    kit.store()
        .list_events(room, ListEventsOptions::default())
        .await
        .unwrap()
        .iter()
        .filter(|row| &row.source.channel_id == agent && row.r#type == EventType::Message)
        .filter_map(|row| {
            if let EventContent::Text(text) = &row.content {
                Some(text.body.clone())
            } else {
                None
            }
        })
        .collect()
}

async fn a_room_turn(loop_back: bool, served_elsewhere: bool) {
    let Some(url) = support::server("a room's turn across two processes") else {
        return;
    };
    let prefix = support::prefix("room-turn");
    let node = support::node(&url, &prefix, loop_back);
    let peer = support::node(&url, &prefix, loop_back);
    let serving = if served_elsewhere { &peer } else { &node };
    serving.serve(worker("worker")).await.unwrap();
    let kit = RoomKit::new(RoomKitOptions {
        turn_backend: Some(TurnBackendHandle::new(Arc::new(node.clone()))),
        ..RoomKitOptions::default()
    });
    let room: RoomId = "r".parse().unwrap();
    let agent: ChannelId = "agent".parse().unwrap();
    let text: ChannelId = "text".parse().unwrap();
    kit.register_channel(RecordingTransport::new(
        text.clone(),
        ChannelType::SMS,
        RecordingTransportOptions::default(),
    ))
    .unwrap();
    kit.register_channel(ChannelDeclaration {
        identity: Some(AgentIdentity::default()),
        ..ChannelDeclaration::new(agent.clone())
    })
    .unwrap();
    kit.create_room(CreateRoomOptions {
        room_id: Some(room.clone()),
        ..CreateRoomOptions::default()
    })
    .await
    .unwrap();
    kit.attach_channel(&room, &text, AttachChannelOptions::default())
        .await
        .unwrap();
    kit.attach_channel(
        &room,
        &agent,
        AttachChannelOptions {
            category: Some(ChannelCategory::Intelligence),
            ..AttachChannelOptions::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(replies(&kit, &room, &text, &agent).await, ["worker"]);

    // A consumed message leaves its stream behind: the owner's inbox exists
    // once a turn went through it.
    let mut redis = support::connection(&url).await;
    let inbox: bool = redis::cmd("EXISTS")
        .arg(format!("{prefix}:inbox:{}", node.node_id()))
        .query_async(&mut redis)
        .await
        .unwrap();
    assert_eq!(
        inbox,
        loop_back || served_elsewhere,
        "a turn served by its owner's process uses the inbox only looped back"
    );
    // Every process reads who holds the room's state.
    let holder = peer.registry().holder("agent", "r").await.unwrap();
    assert_eq!(holder, Some(format!("{}:worker", serving.node_id())));
    // A detach leaves nobody holding it.
    kit.detach_channel(&room, &agent, DetachChannelOptions::default())
        .await
        .unwrap();
    assert_eq!(peer.registry().holder("agent", "r").await.unwrap(), None);
    kit.close().await.unwrap();
    peer.close().await.unwrap();
}

#[tokio::test]
async fn a_room_turn_runs_on_the_executor_another_process_serves() {
    a_room_turn(false, true).await;
}

#[tokio::test]
async fn a_room_turn_runs_on_the_executor_its_owners_process_serves() {
    a_room_turn(false, false).await;
}

#[tokio::test]
async fn loop_back_carries_a_locally_served_turn_through_the_inbox() {
    a_room_turn(true, false).await;
}
