//! A room with an SMS transport, a web chat and the agent, which calls a
//! local fake of the endpoint: the provider under `RoomKit`'s own tool loop,
//! which runs the tools and sends each round's results back.

use std::time::Duration;

use roomkit::model::hook::AllowOptions;
use roomkit::model::{
    ChannelId, ChannelType, HookResult, InboundMessage, RoomEvent, RoomId, TextContent, Timestamp,
};
use roomkit::store::ListEventsOptions;
use roomkit::testing::{
    RecordingTransport, RecordingTransportOptions, SequentialEnvironment, kit_with_environment,
};
use roomkit::{
    AttachChannelOptions, ConversationStore, CreateRoomOptions, EventFilter, HookExecution,
    HookOptions, HookTrigger, InboundResult, ProcessInboundOptions, RoomKit, RoomKitOptions,
};
use roomkit_ai::{AiChannel, AiChannelOptions};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::support::{KEY, chunk, ending, events, json_reply, stream_reply};
use crate::replay::fake::{Body, Ending, Fake, Reply};
use crate::{OpenAi, OpenAiConfig};

pub(crate) const SYSTEM_PROMPT: &str = "You answer a bakery's customers, in a few words.";

pub(crate) fn channel(id: &str) -> ChannelId {
    ChannelId::new(id).unwrap()
}

fn room_id() -> RoomId {
    RoomId::new("room_1").unwrap()
}

/// A streamed completion that says `text`: what the agent, which asks for
/// a stream, reads an answer from.
pub(crate) fn streamed_saying(text: &str) -> Reply {
    let [end, usage] = ending("stop", 12, 3);
    let said = chunk(&json!({"role": "assistant", "content": text}));
    events(&[said, end, usage])
}

/// Raw SSE data, including streams that close without a terminal event.
pub(crate) fn sse(body: &str) -> Reply {
    stream_reply(vec![body.as_bytes().to_vec()], Ending::Ends)
}

/// An answer of HTTP `status` holding `body`, with the header `name`.
pub(crate) fn with_header(status: u16, body: &str, name: &str, value: &str) -> Reply {
    let Reply::Answer { mut headers, .. } = json_reply(status, body) else {
        unreachable!("a JSON reply is an answer");
    };
    headers.push((name.to_owned(), value.to_owned()));
    Reply::Answer {
        status,
        headers,
        body: Body::Whole(body.as_bytes().to_vec()),
    }
}

/// A request the fake received.
pub(crate) struct Sent {
    /// The request's path.
    pub(crate) path: String,
    /// Its `Authorization` header, if any.
    pub(crate) authorization: Option<String>,
    /// Its body, as JSON.
    pub(crate) body: Value,
}

/// A room with an SMS transport, a web chat and the agent, which calls the
/// fake.
pub(crate) struct Room {
    pub(crate) kit: RoomKit,
    pub(crate) sms: RecordingTransport,
    fake: Fake,
}

impl Room {
    /// The room, its agent's configuration adjusted by `adjust`.
    pub(crate) async fn configured(
        replies: Vec<Reply>,
        api_key: &str,
        adjust: impl FnOnce(OpenAiConfig) -> OpenAiConfig,
    ) -> Self {
        Self::built(replies, api_key, adjust, |options| options).await
    }

    /// The room, its agent's configuration adjusted by `adjust` and its
    /// channel's options by `equip`. The agent calls the fake as a server
    /// behind a base URL.
    pub(crate) async fn built(
        replies: Vec<Reply>,
        api_key: &str,
        adjust: impl FnOnce(OpenAiConfig) -> OpenAiConfig,
        equip: impl FnOnce(AiChannelOptions) -> AiChannelOptions,
    ) -> Self {
        let fake = Fake::serving(replies).await;
        let config = adjust(OpenAiConfig {
            base_url: Some(format!("http://{}/v1", fake.address)),
            ..OpenAiConfig::new(api_key, "fake-model")
        });
        let provider = OpenAi::new(config).unwrap();
        let kit = kit_with_environment(
            RoomKitOptions::default(),
            SequentialEnvironment::new(Timestamp::from_unix_millis(1_792_677_780_000).unwrap()),
        );
        let sms = RecordingTransport::new(
            channel("sms"),
            ChannelType::SMS,
            RecordingTransportOptions::default(),
        );
        let web = RecordingTransport::new(
            channel("web"),
            ChannelType::WEBSOCKET,
            RecordingTransportOptions::default(),
        );
        let options = equip(AiChannelOptions {
            system_prompt: Some(SYSTEM_PROMPT.into()),
            ..AiChannelOptions::default()
        });
        let agent = AiChannel::new(channel("assistant"), provider, options).unwrap();
        kit.register_channel(sms.clone()).unwrap();
        kit.register_channel(web).unwrap();
        kit.register_channel(agent).unwrap();
        kit.create_room(CreateRoomOptions {
            room_id: Some(room_id()),
            ..Default::default()
        })
        .await
        .unwrap();
        for id in ["sms", "web", "assistant"] {
            kit.attach_channel(&room_id(), &channel(id), AttachChannelOptions::default())
                .await
                .unwrap();
        }
        Self { kit, sms, fake }
    }

    /// `text` from `sender` on the SMS transport.
    pub(crate) async fn says(&self, text: &str) -> InboundResult {
        let message = InboundMessage::new(
            self.sms.channel_id().clone(),
            "alice",
            TextContent::new(text),
        );
        let options = ProcessInboundOptions {
            room_id: Some(room_id()),
            ..Default::default()
        };
        self.kit.process_inbound(message, options).await.unwrap()
    }

    /// Every row of the room's timeline, the refused ones included.
    pub(crate) async fn timeline(&self) -> Vec<RoomEvent> {
        let every_row = ListEventsOptions {
            limit: u64::MAX,
            event_filter: Some(EventFilter {
                include_blocked: true,
                ..EventFilter::default()
            }),
            ..ListEventsOptions::default()
        };
        let store = self.kit.store();
        store.list_events(&room_id(), every_row).await.unwrap()
    }

    /// The requests the fake received, in order.
    pub(crate) fn requests(&self) -> Vec<Sent> {
        let seen = self.fake.seen().into_iter();
        seen.map(|seen| Sent {
            path: seen.target,
            authorization: seen.headers.get("authorization").cloned(),
            body: serde_json::from_slice(&seen.body).unwrap_or(Value::Null),
        })
        .collect()
    }
}

/// The key every test configures, as the bearer a request carries it in.
pub(crate) fn bearer_of_the_key() -> String {
    format!("Bearer {KEY}")
}

/// The events `ON_ERROR` fires with in `room`, as an observer receives them.
pub(crate) fn on_error(room: &Room) -> mpsc::UnboundedReceiver<RoomEvent> {
    let (errors, reported) = mpsc::unbounded_channel();
    let spec = HookOptions {
        execution: HookExecution::Async,
        name: "errors".to_owned(),
        ..Default::default()
    };
    room.kit
        .hook(
            HookTrigger::OnError,
            move |event: RoomEvent, _| {
                errors.send(event).unwrap();
                async { Ok(HookResult::allow(AllowOptions::default())) }
            },
            spec,
        )
        .unwrap();
    reported
}

/// The next event `reported`, or a failure after a second.
pub(crate) async fn next(reported: &mut mpsc::UnboundedReceiver<RoomEvent>) -> RoomEvent {
    tokio::time::timeout(Duration::from_secs(1), reported.recv())
        .await
        .unwrap()
        .unwrap()
}
