//! The time to first token, the metric `roomkit.llm.ttfb_ms`: Python's
//! `FirstToken` in `AnthropicAIProvider._events`, read at the first output
//! `is_first_output` names.

use std::sync::Arc;

use roomkit::model::hook::AllowOptions;
use roomkit::model::{
    ChannelId, ChannelType, HookResult, InboundMessage, Metadata, RoomEvent, RoomId, TextContent,
};
use roomkit::telemetry::{MockTelemetryProvider, TelemetryConfig};
use roomkit::testing::{RecordingTransport, RecordingTransportOptions};
use roomkit::{
    AttachChannelOptions, CreateRoomOptions, HookOptions, HookTrigger, ProcessInboundOptions,
    RoomKit, RoomKitOptions,
};
use roomkit_ai::AiProvider;
use roomkit_providers_client::items::next;
use serde_json::json;

use super::support::{elsewhere, end, hello, pointed, saying, start, stream_of};
use crate::Anthropic;
use crate::replay::fake::{Fake, Reply};

/// How a generation is read.
#[derive(Clone, Copy, Debug)]
enum Mode {
    Generate,
    Stream,
}

fn channel(id: &str) -> ChannelId {
    ChannelId::new(id).unwrap()
}

fn room_id() -> RoomId {
    RoomId::new("room_1").unwrap()
}

/// Each `roomkit.llm.ttfb_ms` one generation records, its value, unit and
/// attributes: asked as `mode`, answered `reply`, inside a hook of a kit
/// reporting to a mock, as a turn's generation runs under its kit.
async fn recorded(mode: Mode, reply: Reply) -> Vec<(f64, String, Metadata)> {
    let fake = Fake::serving(vec![reply]).await;
    let provider = Arc::new(pointed(elsewhere("fake-model"), &fake));
    let mock = Arc::new(MockTelemetryProvider::default());
    let kit = RoomKit::new(RoomKitOptions {
        telemetry: Some(TelemetryConfig::from(Arc::clone(&mock))),
        ..RoomKitOptions::default()
    });
    let sms = RecordingTransport::new(
        channel("sms"),
        ChannelType::SMS,
        RecordingTransportOptions::default(),
    );
    kit.register_channel(sms).unwrap();
    let create = CreateRoomOptions {
        room_id: Some(room_id()),
        ..CreateRoomOptions::default()
    };
    kit.create_room(create).await.unwrap();
    kit.attach_channel(&room_id(), &channel("sms"), AttachChannelOptions::default())
        .await
        .unwrap();
    kit.hook(
        HookTrigger::BeforeBroadcast,
        move |_: RoomEvent, _| {
            let provider = Arc::clone(&provider);
            async move {
                generate(mode, &provider).await;
                HookResult::allow(AllowOptions::default())
            }
        },
        HookOptions {
            name: "generates".to_owned(),
            ..HookOptions::default()
        },
    )
    .unwrap();
    let message = InboundMessage::new(channel("sms"), "+15550000001", TextContent::new("Hello"));
    let options = ProcessInboundOptions {
        room_id: Some(room_id()),
        ..ProcessInboundOptions::default()
    };
    kit.process_inbound(message, options).await.unwrap();
    kit.close().await.unwrap();
    mock.metrics()
        .into_iter()
        .filter(|(name, ..)| name == "roomkit.llm.ttfb_ms")
        .map(|(_, value, unit, attributes)| (value, unit, attributes))
        .collect()
}

/// One generation, read to its end whatever it comes to.
async fn generate(mode: Mode, provider: &Anthropic) {
    let context = hello();
    match mode {
        Mode::Generate => {
            drop(provider.generate(&context).await);
        }
        Mode::Stream => {
            let mut events = provider.generate_structured_stream(&context);
            while next(&mut events).await.is_some() {}
        }
    }
}

// A whole answer reads the stream every call gives, so both modes record
// the measure once, at the first text, under the provider's own name.
#[tokio::test]
async fn the_time_to_first_token_is_recorded_once_on_both_modes() {
    let mut labels = Metadata::new();
    labels.insert("provider".to_owned(), json!("AnthropicAIProvider"));
    labels.insert("model".to_owned(), json!("fake-model"));
    for mode in [Mode::Generate, Mode::Stream] {
        let metrics = recorded(mode, saying("Ok.")).await;
        let [(value, unit, attributes)] = &metrics[..] else {
            panic!("one metric in {metrics:?} for {mode:?}");
        };
        assert!(*value >= 0.0);
        assert_eq!(unit, "ms");
        assert_eq!(attributes, &labels);
    }
}

// Python: providers/anthropic/stream_events.py `is_first_output`: reasoning
// that says something is output, a signature alone is not, and a call's
// fragments never are.
#[tokio::test]
async fn reasoning_records_the_first_token_and_a_call_alone_records_none() {
    let reasoned = [
        start(),
        json!({"type": "content_block_start", "index": 0,
            "content_block": {"type": "thinking", "thinking": ""}}),
        json!({"type": "content_block_delta", "index": 0,
            "delta": {"type": "thinking_delta", "thinking": "Hm."}}),
        json!({"type": "content_block_delta", "index": 0,
            "delta": {"type": "signature_delta", "signature": "c2ln"}}),
        json!({"type": "content_block_stop", "index": 0}),
    ];
    let reasoned = [&reasoned[..], &end("end_turn")].concat();
    let metrics = recorded(Mode::Stream, stream_of(&reasoned)).await;
    assert_eq!(metrics.len(), 1, "{metrics:?}");

    let called = [
        start(),
        json!({"type": "content_block_start", "index": 0,
            "content_block": {"type": "tool_use", "id": "toolu_1", "name": "lookup", "input": {}}}),
        json!({"type": "content_block_delta", "index": 0,
            "delta": {"type": "input_json_delta", "partial_json": "{}"}}),
        json!({"type": "content_block_stop", "index": 0}),
    ];
    let called = [&called[..], &end("tool_use")].concat();
    let metrics = recorded(Mode::Stream, stream_of(&called)).await;
    assert_eq!(metrics, vec![]);
}
