//! DeepSeek, Kimi, MiniMax, xAI, Cerebras, Qwen and Meta against the wire cases captured on their
//! `RoomKit` Python providers, each a derivative of `OpenAIAIProvider`:
//! `tests/fixtures/python_<vendor>_cases.json`, written by
//! `scripts/generate_openai_chat_cases.py` from `scripts/vendor_chat_cases.py`.
//! Each case is replayed by the harness through a local fake of the
//! endpoint, as `OpenAI`'s are, and what the provider sends and gives is
//! compared with Python's. A case the provider answers otherwise is named,
//! with what it does instead.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_client::Client;
use roomkit_providers_client::harness::named::Named;
use roomkit_providers_client::harness::{Fixture, Replays, replayed_or_named};
use serde_json::Value;

use crate::{
    Cerebras, CerebrasConfig, DeepSeek, DeepSeekConfig, Kimi, KimiConfig, Meta, MetaConfig,
    MiniMax, MiniMaxConfig, Qwen, QwenConfig, Xai, XaiConfig,
};

/// The fixture's clock, in seconds since 1970: 2026-01-01T00:00:00Z.
const CLOCK: f64 = 1_767_225_600.0;

macro_rules! replays {
    ($provider:ident, $config:ident) => {
        impl Replays for $provider {
            const KEY_HEADER: &'static str = "authorization";

            fn built(config: &Value) -> Result<Self, ProviderError> {
                let config: $config = serde_json::from_value(config.clone()).unwrap();
                Self::new(config)
            }

            fn client(&mut self) -> &mut Client {
                &mut self.openai_mut().client
            }

            fn point_at(&mut self, fake: SocketAddr) {
                self.openai_mut().point_at(fake);
            }
        }
    };
}

replays!(DeepSeek, DeepSeekConfig);
replays!(Kimi, KimiConfig);
replays!(MiniMax, MiniMaxConfig);
replays!(Xai, XaiConfig);
replays!(Cerebras, CerebrasConfig);
replays!(Qwen, QwenConfig);
replays!(Meta, MetaConfig);

/// No case of the seven is answered otherwise than Python.
const NONE_NAMED: Named = Named {
    questioned_otherwise: &[],
    only_python: &[],
    by_its_own_rule: &[],
};

fn fixture(text: &str) -> Fixture {
    let fixture = Fixture::read(text);
    assert!((fixture.clock() - CLOCK).abs() < f64::EPSILON);
    fixture
}

/// Every case of `text`'s fixture whose call is `call`, replayed on `P`: how
/// many replayed as Python, and how many are named.
async fn replayed<P: Replays>(text: &str, call: &str) -> (usize, usize) {
    replayed_or_named::<P>(&fixture(text), &NONE_NAMED, call).await
}

const DEEPSEEK: &str = include_str!("../../tests/fixtures/python_deepseek_cases.json");
const KIMI: &str = include_str!("../../tests/fixtures/python_kimi_cases.json");
const MINIMAX: &str = include_str!("../../tests/fixtures/python_minimax_cases.json");
const XAI: &str = include_str!("../../tests/fixtures/python_xai_cases.json");
const CEREBRAS: &str = include_str!("../../tests/fixtures/python_cerebras_cases.json");
const QWEN: &str = include_str!("../../tests/fixtures/python_qwen_cases.json");
const META: &str = include_str!("../../tests/fixtures/python_meta_cases.json");

#[tokio::test]
async fn every_deepseek_case_replays_as_python() {
    let generated = replayed::<DeepSeek>(DEEPSEEK, "generate").await;
    let streamed = replayed::<DeepSeek>(DEEPSEEK, "generate_structured_stream").await;
    let listed = replayed::<DeepSeek>(DEEPSEEK, "list_models").await;
    assert_eq!((generated, streamed, listed), ((49, 0), (1, 0), (1, 0)));
}

#[tokio::test]
async fn every_kimi_case_replays_as_python() {
    let generated = replayed::<Kimi>(KIMI, "generate").await;
    let streamed = replayed::<Kimi>(KIMI, "generate_structured_stream").await;
    let listed = replayed::<Kimi>(KIMI, "list_models").await;
    assert_eq!((generated, streamed, listed), ((42, 0), (2, 0), (1, 0)));
}

#[tokio::test]
async fn every_minimax_case_replays_as_python() {
    let generated = replayed::<MiniMax>(MINIMAX, "generate").await;
    let streamed = replayed::<MiniMax>(MINIMAX, "generate_structured_stream").await;
    let listed = replayed::<MiniMax>(MINIMAX, "list_models").await;
    assert_eq!((generated, streamed, listed), ((47, 0), (1, 0), (1, 0)));
}

#[tokio::test]
async fn every_xai_case_replays_as_python() {
    let generated = replayed::<Xai>(XAI, "generate").await;
    let streamed = replayed::<Xai>(XAI, "generate_structured_stream").await;
    let listed = replayed::<Xai>(XAI, "list_models").await;
    assert_eq!((generated, streamed, listed), ((46, 0), (1, 0), (1, 0)));
}

#[tokio::test]
async fn every_cerebras_case_replays_as_python() {
    let generated = replayed::<Cerebras>(CEREBRAS, "generate").await;
    let streamed = replayed::<Cerebras>(CEREBRAS, "generate_structured_stream").await;
    let listed = replayed::<Cerebras>(CEREBRAS, "list_models").await;
    assert_eq!((generated, streamed, listed), ((56, 0), (3, 0), (1, 0)));
}

#[tokio::test]
async fn every_qwen_case_replays_as_python() {
    let generated = replayed::<Qwen>(QWEN, "generate").await;
    let streamed = replayed::<Qwen>(QWEN, "generate_structured_stream").await;
    let listed = replayed::<Qwen>(QWEN, "list_models").await;
    assert_eq!((generated, streamed, listed), ((47, 0), (1, 0), (2, 0)));
}

#[tokio::test]
async fn every_meta_case_replays_as_python() {
    let generated = replayed::<Meta>(META, "generate").await;
    let streamed = replayed::<Meta>(META, "generate_structured_stream").await;
    let listed = replayed::<Meta>(META, "list_models").await;
    assert_eq!((generated, streamed, listed), ((41, 0), (1, 0), (2, 0)));
}
