//! A room's work as one trace on OpenTelemetry.
//!
//! A room of an SMS transport and an AI agent, its kit reporting to an
//! [`OpenTelemetryProvider`] whose SDK tracer provider exports to memory.
//! One message: its inbound, the broadcast it starts and the agent's turn
//! under it, and the turn's generation, each a span of the same trace,
//! parents before children. It prints each span and checks they share one
//! trace.
//!
//! ```sh
//! cargo run -p roomkit-providers-opentelemetry --example one_trace
//! ```
#![expect(clippy::unwrap_used, reason = "an example stops on a broken fixture")]
#![expect(clippy::print_stdout, reason = "an example shows what it did")]

use std::collections::HashSet;
use std::sync::Arc;

use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
use roomkit::model::{
    ChannelCategory, ChannelId, ChannelType, InboundMessage, RoomId, TextContent,
};
use roomkit::testing::{RecordingTransport, RecordingTransportOptions};
use roomkit::{
    AttachChannelOptions, CreateRoomOptions, ProcessInboundOptions, RoomKit, RoomKitOptions,
};
use roomkit_ai::{AiChannel, AiChannelOptions, MockAiProvider, MockAiProviderOptions};
use roomkit_providers_opentelemetry::{OpenTelemetryProvider, OpenTelemetryProviderOptions};

#[tokio::main]
async fn main() {
    let exporter = InMemorySpanExporter::default();
    let tracer_provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let telemetry = OpenTelemetryProvider::new(OpenTelemetryProviderOptions {
        tracer_provider: Some(tracer_provider),
        service_name: "one-trace".to_owned(),
        ..OpenTelemetryProviderOptions::default()
    });
    let kit = RoomKit::new(RoomKitOptions {
        telemetry: Some(Arc::new(telemetry).into()),
        ..RoomKitOptions::default()
    });
    let sms = ChannelId::new("sms").unwrap();
    let agent = ChannelId::new("agent").unwrap();
    let transport = RecordingTransport::new(
        sms.clone(),
        ChannelType::SMS,
        RecordingTransportOptions::default(),
    );
    kit.register_channel(transport).unwrap();
    let model = MockAiProvider::new(MockAiProviderOptions {
        responses: vec!["Hello, how can I help?".to_owned()],
        ..MockAiProviderOptions::default()
    });
    kit.register_channel(
        AiChannel::new(agent.clone(), model, AiChannelOptions::default()).unwrap(),
    )
    .unwrap();
    let room = RoomId::new("support").unwrap();
    let create = CreateRoomOptions {
        room_id: Some(room.clone()),
        ..CreateRoomOptions::default()
    };
    kit.create_room(create).await.unwrap();
    kit.attach_channel(&room, &sms, AttachChannelOptions::default())
        .await
        .unwrap();
    let intelligence = AttachChannelOptions {
        category: Some(ChannelCategory::Intelligence),
        ..AttachChannelOptions::default()
    };
    kit.attach_channel(&room, &agent, intelligence)
        .await
        .unwrap();

    let message = InboundMessage::new(sms, "ana", TextContent::new("Hi"));
    let options = ProcessInboundOptions {
        room_id: Some(room),
        ..ProcessInboundOptions::default()
    };
    kit.process_inbound(message, options).await.unwrap();
    kit.close().await.unwrap();

    let spans = exporter.get_finished_spans().unwrap();
    for span in &spans {
        println!(
            "{:<24} trace {} span {} parent {}",
            span.name,
            span.span_context.trace_id(),
            span.span_context.span_id(),
            span.parent_span_id
        );
    }
    let traces: HashSet<_> = spans
        .iter()
        .map(|span| span.span_context.trace_id())
        .collect();
    assert!(
        spans.len() >= 3,
        "the inbound, the broadcast and the turn: {spans:?}"
    );
    assert_eq!(traces.len(), 1, "one trace for the message's work");
    println!("{} spans, one trace", spans.len());
}
