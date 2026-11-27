//! The vendor providers of the `OpenAI` wire on a fake of it: what a host
//! writes to select a vendor's dialect, on the vendor's API or on a server
//! of its own, and what each request then carries.
//! Python: `tests/test_providers/test_minimax_kimi.py` and `test_deepseek.py`.

use roomkit_ai::{
    AiChannelOptions, AiContext, AiMessage, AiProvider, AiRole, AiTool, StreamEvent, ToolResult,
    Tools,
};
use serde_json::{Value, json};

use super::room::Room;
use super::support::{KEY, body_of, chunk, ending, events, json_reply, saying};
use crate::replay::drained;
use crate::replay::fake::{Fake, Reply};
use crate::{
    AzureAi, AzureAiConfig, DeepSeek, DeepSeekConfig, Kimi, KimiConfig, Meta, MetaConfig, MiniMax,
    MiniMaxConfig, OpenAi, OpenAiConfig,
};

/// A round that reasons, says it looks and calls; a round that only calls;
/// the answer; then the next turn's answer.
fn rounds() -> Vec<Reply> {
    let call = |id: &str| {
        chunk(
            &json!({ "tool_calls": [{ "index": 0, "id": id, "type": "function",
            "function": { "name": "get_weather", "arguments": "{}" } }] }),
        )
    };
    let mut first = vec![
        chunk(&json!({ "reasoning_content": "WHY." })),
        chunk(&json!({ "content": "Let me look." })),
        call("c1"),
    ];
    first.extend(ending("tool_calls", 10, 5));
    let mut second = vec![call("c2")];
    second.extend(ending("tool_calls", 10, 5));
    let mut answer = vec![
        chunk(&json!({ "reasoning_content": "DONE." })),
        chunk(&json!({ "content": "Sunny." })),
    ];
    answer.extend(ending("stop", 10, 5));
    let mut later = vec![chunk(&json!({ "content": "You are welcome." }))];
    later.extend(ending("stop", 10, 5));
    let rounds = [first, second, answer, later];
    rounds.iter().map(|chunks| events(chunks)).collect()
}

fn weather() -> Tools {
    let tool = AiTool {
        parameters: json!({ "type": "object" }),
        ..AiTool::new("get_weather", "The weather.").unwrap()
    };
    Tools::new().tool(tool, |_call, _context| async move {
        Ok(ToolResult::Text("sunny".to_owned()))
    })
}

/// Each assistant message of a request: its text, and its reasoning field.
fn reasoning(request: &Value) -> Vec<(Value, Value)> {
    let messages = request["messages"].as_array().unwrap().iter();
    messages
        .filter(|message| message["role"] == "assistant")
        .map(|message| {
            let field = message.get("reasoning_content").cloned();
            (
                message["content"].clone(),
                field.unwrap_or_else(|| "absent".into()),
            )
        })
        .collect()
}

/// Two turns through a room: the first with its two tool rounds.
async fn two_turns<P: AiProvider + 'static>(provider: impl FnOnce(&str) -> P) -> Vec<Value> {
    let equip = |options| AiChannelOptions {
        tools: weather().into(),
        ..options
    };
    let room = Room::with_provider(rounds(), provider, equip).await;
    room.says("go").await;
    room.says("thanks").await;
    let requests = room.requests().into_iter();
    requests.map(|request| request.body).collect()
}

// Python: tests/test_providers/test_minimax_kimi.py::test_a_host_selects_the_dialect_by_the_class_on_a_server_of_its_own
/// The end-to-end proof: a room, an AI channel, the class chosen, a
/// generic server's URL. A round's reasoning goes back in the field for as
/// long as the turn lasts, then no more; no cap is sent unless one is set.
#[tokio::test]
async fn a_host_selects_the_dialect_by_the_class_on_a_server_of_its_own() {
    let kimi = two_turns(kimi("moonshotai/Kimi-K3")).await;
    let minimax = two_turns(minimax("MiniMaxAI/MiniMax-M3")).await;
    for (requests, empty) in [(&kimi, json!(" ")), (&minimax, json!("absent"))] {
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[0].get("max_tokens"), None);
        assert_eq!(requests[0].get("max_completion_tokens"), None);
        assert_eq!(
            reasoning(&requests[2]),
            [
                (json!("Let me look."), json!("WHY.")),
                (Value::Null, empty.clone())
            ]
        );
        // The round's text and its calls, one message as the turn sent it.
        assert_eq!(
            reasoning(&requests[3]),
            [
                (json!("Let me look."), empty.clone()),
                (Value::Null, empty.clone()),
                (json!("Sunny."), json!("absent")),
            ]
        );
    }
}

/// The base URL of `fake`, as a host writes a server of its own.
fn url_of(fake: &Fake) -> String {
    format!("http://{}/v1", fake.address)
}

/// One generation's request through `provider`, on the fake.
async fn sent<P: AiProvider>(
    provider: impl FnOnce(&str) -> P,
    context: AiContext,
) -> (Value, Result<(), String>) {
    let fake = Fake::serving(vec![saying("Hello.")]).await;
    let provider = provider(&url_of(&fake));
    let answered = provider.generate(&context).await;
    let answered = answered
        .map(|_| ())
        .map_err(|error| format!("{} {}", error.provider, error.message));
    let body = if fake.seen().is_empty() {
        Value::Null
    } else {
        body_of(&fake, 0)
    };
    (body, answered)
}

fn hello() -> AiContext {
    AiContext {
        temperature: 0.2,
        messages: vec![AiMessage::new(AiRole::User, "Hi")],
        ..AiContext::default()
    }
}

/// The members of `body` among `keys`.
fn members(body: &Value, keys: &[&str]) -> Value {
    let picked = keys.iter().filter_map(|key| {
        let value = body.get(*key)?;
        Some(((*key).to_owned(), value.clone()))
    });
    Value::Object(picked.collect())
}

/// Kimi's provider for `model` on a server's URL.
fn kimi(model: &'static str) -> impl FnOnce(&str) -> Kimi {
    move |url: &str| {
        Kimi::new(KimiConfig {
            base_url: url.to_owned(),
            ..KimiConfig::new(KEY, model)
        })
        .unwrap()
    }
}

/// MiniMax's provider for `model` on a server's URL.
fn minimax(model: &'static str) -> impl FnOnce(&str) -> MiniMax {
    move |url: &str| {
        MiniMax::new(MiniMaxConfig {
            base_url: url.to_owned(),
            ..MiniMaxConfig::new(KEY, model)
        })
        .unwrap()
    }
}

/// DeepSeek's provider on a server's URL.
fn deepseek(url: &str) -> DeepSeek {
    DeepSeek::new(DeepSeekConfig {
        base_url: url.to_owned(),
        ..DeepSeekConfig::new(KEY, "deepseek-v4-pro")
    })
    .unwrap()
}

/// DeepSeek's provider on a server's URL, sending no temperature.
fn deepseek_without_temperature(url: &str) -> DeepSeek {
    DeepSeek::new(DeepSeekConfig {
        base_url: url.to_owned(),
        supports_custom_temperature: false,
        ..DeepSeekConfig::new(KEY, "deepseek-v4-pro")
    })
    .unwrap()
}

/// A request declaring one tool named `name`.
fn declaring(name: &str, context: AiContext) -> AiContext {
    AiContext {
        tools: vec![AiTool {
            parameters: json!({ "type": "object" }),
            ..AiTool::new(name, "A tool.").unwrap()
        }],
        ..context
    }
}

/// A turn stating `effort` and `enable`.
fn turn(effort: Option<&str>, enable: Option<bool>) -> AiContext {
    AiContext {
        reasoning_effort: effort.map(str::to_owned),
        enable_thinking: enable,
        ..hello()
    }
}

// Python: tests/test_providers/test_minimax_kimi.py::test_what_a_turn_states_of_its_reasoning
#[tokio::test]
async fn what_a_turn_states_of_its_reasoning() {
    let keys = ["reasoning_effort", "thinking", "temperature"];
    let with_tool = |context| declaring("get_weather", context);
    let cases = [
        // Each cannot stop reasoning: a turn that states off gets the least it takes.
        (
            sent(kimi("kimi-k3.1-pro"), turn(None, Some(false))).await,
            json!({ "reasoning_effort": "low" }),
        ),
        (
            sent(kimi("kimi-k3"), turn(Some("none"), None)).await,
            json!({ "reasoning_effort": "low" }),
        ),
        (
            sent(minimax("MiniMax-M3.1-Flash"), turn(None, Some(false))).await,
            json!({ "reasoning_effort": "low" }),
        ),
        // M3 can, through MiniMax's own switch.
        (
            sent(minimax("MiniMax-M3"), turn(None, Some(false))).await,
            json!({ "thinking": { "type": "disabled" } }),
        ),
        // A level the model does not take goes as the nearest it takes.
        (
            sent(kimi("kimi-k3.1-pro"), turn(Some("medium"), None)).await,
            json!({ "reasoning_effort": "low" }),
        ),
        (
            sent(kimi("kimi-k3"), turn(Some("xhigh"), None)).await,
            json!({ "reasoning_effort": "high" }),
        ),
        (
            sent(minimax("MiniMax-M3.1-Flash"), turn(Some("minimal"), None)).await,
            json!({ "reasoning_effort": "low" }),
        ),
        // A model the catalogue does not name: what the turn states goes as it is.
        (
            sent(kimi("moonshotai/Kimi-K3"), turn(Some("medium"), None)).await,
            json!({ "reasoning_effort": "medium" }),
        ),
        (
            sent(kimi("moonshotai/Kimi-K3"), turn(None, Some(false))).await,
            json!({}),
        ),
        (
            sent(minimax("MiniMaxAI/MiniMax-M3"), turn(Some("none"), None)).await,
            json!({ "reasoning_effort": "none" }),
        ),
        // A turn with tools sends its effort too, where the generic provider sends none.
        (
            sent(kimi("kimi-k3"), with_tool(turn(Some("high"), None))).await,
            json!({ "reasoning_effort": "high" }),
        ),
        (
            sent(
                minimax("MiniMax-M3.1-Flash"),
                with_tool(turn(Some("high"), None)),
            )
            .await,
            json!({ "reasoning_effort": "high" }),
        ),
        (
            sent(
                deepseek_without_temperature,
                with_tool(turn(Some("high"), None)),
            )
            .await,
            json!({ "thinking": { "type": "enabled", "reasoning_effort": "high" } }),
        ),
    ];
    for ((body, answered), expected) in cases {
        assert_eq!(answered, Ok(()));
        assert_eq!(members(&body, &keys), expected);
    }
}

// Python: tests/test_providers/test_minimax_kimi.py::test_the_vendor_s_own_endpoint_is_the_default
// and ::test_the_temperature_goes_where_the_server_takes_it
#[test]
fn each_vendor_s_endpoint_cap_and_temperature_are_python_s() {
    let kimi = KimiConfig::new(KEY, "kimi-k3");
    let minimax = MiniMaxConfig::new(KEY, "MiniMax-M3");
    let deepseek = DeepSeekConfig::new(KEY, "deepseek-v4-pro");
    assert_eq!(
        [
            (
                kimi.base_url.as_str(),
                kimi.max_tokens,
                kimi.use_max_completion_tokens,
                kimi.supports_custom_temperature
            ),
            (
                minimax.base_url.as_str(),
                minimax.max_tokens,
                minimax.use_max_completion_tokens,
                minimax.supports_custom_temperature
            ),
            (
                deepseek.base_url.as_str(),
                deepseek.max_tokens,
                deepseek.use_max_completion_tokens,
                deepseek.supports_custom_temperature
            ),
        ],
        [
            ("https://api.moonshot.ai/v1", None, true, false),
            ("https://api.minimax.io/v1", None, true, false),
            ("https://api.deepseek.com/v1", Some(1024), false, true),
        ]
    );
    let names = [
        Kimi::new(kimi).unwrap().name().to_owned(),
        MiniMax::new(minimax).unwrap().name().to_owned(),
        DeepSeek::new(deepseek).unwrap().name().to_owned(),
    ];
    assert_eq!(names, ["kimi", "minimax", "DeepSeekAIProvider"]);
}

#[tokio::test]
async fn a_cap_goes_under_the_field_the_vendor_reads() {
    let capped = |url: &str| {
        Kimi::new(KimiConfig {
            base_url: url.to_owned(),
            max_tokens: Some(2000),
            ..KimiConfig::new(KEY, "kimi-k3")
        })
        .unwrap()
    };
    let (kimi, _) = sent(capped, hello()).await;
    let (deepseek, _) = sent(deepseek, hello()).await;
    let keys = ["max_tokens", "max_completion_tokens", "temperature"];
    assert_eq!(
        (members(&kimi, &keys), members(&deepseek, &keys)),
        (
            json!({ "max_completion_tokens": 2000 }),
            json!({ "max_tokens": 1024, "temperature": 0.2 })
        )
    );
}

// Python: tests/test_providers/test_reasoning_settings.py, the configs that
// apply their vendor's own rule refuse a statement of what the server takes.
#[test]
fn a_vendor_s_config_refuses_a_statement_of_its_tool_turns() {
    let kimi = Kimi::new(KimiConfig {
        supports_reasoning_effort_with_tools: Some(true),
        ..KimiConfig::new(KEY, "kimi-k3")
    });
    let minimax = MiniMax::new(MiniMaxConfig {
        supports_reasoning_effort_with_tools: Some(false),
        ..MiniMaxConfig::new(KEY, "MiniMax-M3")
    });
    let deepseek = DeepSeek::new(DeepSeekConfig {
        supports_reasoning_effort_with_tools: Some(true),
        ..DeepSeekConfig::new(KEY, "deepseek-v4-pro")
    });
    let refused = [
        kimi.unwrap_err(),
        minimax.unwrap_err(),
        deepseek.unwrap_err(),
    ];
    let said: Vec<_> = refused
        .iter()
        .map(|error| (error.provider.as_str(), error.before_request))
        .collect();
    assert_eq!(
        said,
        [("kimi", true), ("minimax", true), ("deepseek", true)]
    );
    assert_eq!(
        refused[0].message,
        "KimiConfig takes no supports_reasoning_effort_with_tools: its provider applies its \
         vendor's own rule on a turn with tools"
    );
}

// Python: tests/test_providers/test_chat_request.py::test_a_config_whose_vendor_names_the_field_refuses_another
#[test]
fn a_vendor_s_config_refuses_a_thinking_field() {
    let field = || Some("reasoning_content".to_owned());
    let kimi = Kimi::new(KimiConfig {
        thinking_field: field(),
        ..KimiConfig::new(KEY, "kimi-k3")
    });
    let minimax = MiniMax::new(MiniMaxConfig {
        thinking_field: field(),
        ..MiniMaxConfig::new(KEY, "MiniMax-M3")
    });
    let deepseek = DeepSeek::new(DeepSeekConfig {
        thinking_field: field(),
        ..DeepSeekConfig::new(KEY, "deepseek-v4-pro")
    });
    assert!(kimi.is_err() && minimax.is_err());
    assert_eq!(
        deepseek.unwrap_err().message,
        "DeepSeekConfig takes no thinking_field: its provider sends reasoning where its vendor \
         reads it"
    );
}

// Python: tests/test_providers/test_meta_ai.py::TestConfig::test_a_key_no_header_can_carry_is_refused_without_echoing_it
/// Meta's configuration refuses a key no header can carry when it is built,
/// behind a base URL whose credentials would leave the key unsent too, and
/// never says the key.
#[test]
fn meta_refuses_a_key_no_header_can_carry_without_saying_it() {
    let key = "sk-secret\u{2026}";
    for base_url in ["https://api.meta.ai/v1", "https://user:pass@llm.example/v1"] {
        let refused = Meta::new(MetaConfig {
            base_url: base_url.to_owned(),
            ..MetaConfig::new(key, "muse-spark-1.3")
        })
        .unwrap_err();
        assert!(refused.before_request, "{base_url}");
        assert!(refused.message.contains("non-ASCII"), "{base_url}");
        assert!(!refused.message.contains("sk-secret"), "{base_url}");
    }
    let form = serde_json::from_value::<MetaConfig>(json!({"api_key": key}));
    let said = form.unwrap_err().to_string();
    assert!(said.contains("non-ASCII") && !said.contains("sk-secret"));
}

// Python: the openai SDK raises its own `OpenAIError` ("Missing credentials")
// at each call of `AzureAIProvider` whose key is empty, before any request.
/// An Azure deployment asked with an empty key: each call is refused before
/// any request, with a `ProviderError` in place of the SDK's own error.
#[tokio::test]
async fn azure_refuses_each_call_with_an_empty_key_before_any_request() {
    let fake = Fake::serving(vec![saying("Hello.")]).await;
    let config = AzureAiConfig::new("", "https://acme.openai.azure.com", "chat-prod");
    let mut provider = AzureAi::new(config).unwrap();
    provider.openai_mut().point_at(fake.address);
    let whole = provider.generate(&hello()).await.unwrap_err();
    let streamed = drained(provider.generate_structured_stream(&hello())).await;
    let listed = provider.list_models().await.unwrap_err();
    let (events, Some(streamed)) = streamed else {
        panic!("the stream fails");
    };
    assert_eq!(events.len(), 0);
    for refused in [whole, streamed, listed] {
        assert!(refused.before_request, "{refused:?}");
        assert_eq!(refused.provider, "azure");
        assert!(
            refused.message.starts_with("Missing credentials"),
            "{refused:?}"
        );
    }
    assert!(fake.seen().is_empty());
}

// Python: tests/test_providers/test_chat_request.py::test_a_configured_thinking_field_carries_a_round_s_reasoning_to_the_server
/// A generic provider told the field a server reads: a round's reasoning goes
/// back under that very name.
#[tokio::test]
async fn a_generic_provider_told_the_field_sends_reasoning_under_that_name() {
    let bodies = two_turns(|url: &str| {
        OpenAi::new(OpenAiConfig {
            base_url: Some(url.to_owned()),
            thinking_field: Some("reasoning".to_owned()),
            ..OpenAiConfig::new(KEY, "glm-5.3-flash")
        })
        .unwrap()
    })
    .await;
    let messages = bodies[1]["messages"].as_array().unwrap();
    let mut assistant = messages
        .iter()
        .filter(|message| message["role"] == "assistant");
    let round = assistant.next().unwrap();
    assert_eq!(round["reasoning"], "WHY.");
    assert!(round.get("reasoning_content").is_none());
}

// Python: tests/test_providers/test_minimax_kimi.py::test_a_server_decides_what_a_model_the_catalogue_does_not_name_reads
#[test]
fn a_server_decides_what_a_model_the_catalogue_does_not_name_reads() {
    let kimi = |model: &str, base_url: Option<&str>| {
        let config = KimiConfig::new(KEY, model);
        let base_url = base_url.map_or(config.base_url.clone(), str::to_owned);
        let provider = Kimi::new(KimiConfig { base_url, ..config });
        provider.unwrap().supports_vision()
    };
    let minimax = |model: &str, base_url: Option<&str>| {
        let config = MiniMaxConfig::new(KEY, model);
        let base_url = base_url.map_or(config.base_url.clone(), str::to_owned);
        let provider = MiniMax::new(MiniMaxConfig { base_url, ..config });
        provider.unwrap().supports_vision()
    };
    let deepseek = DeepSeek::new(DeepSeekConfig::new(KEY, "deepseek-v4-pro")).unwrap();
    let server = Some("http://gpu:8000/v1");
    assert_eq!(
        [
            kimi("kimi-k3", None),
            kimi("unknown", None),
            kimi("unknown", server),
            minimax("MiniMax-M3", None),
            minimax("unknown", None),
            minimax("unknown", server),
            deepseek.supports_vision(),
        ],
        [true, false, true, true, false, true, false]
    );
}

// Python: tests/test_providers/test_minimax_kimi.py::test_kimi_s_tool_names_are_checked_before_the_request
// and test_vendor_endpoint.py's deepseek and kimi sites
#[tokio::test]
async fn a_vendor_s_tool_names_are_checked_on_its_own_endpoint_alone() {
    let kimi = |name: &str| {
        let provider = Kimi::new(KimiConfig::new(KEY, "kimi-k3")).unwrap();
        let context = declaring(name, hello());
        async move {
            let refused = provider.generate(&context).await.err();
            refused.map(|error| (error.provider, error.before_request))
        }
    };
    let long = "a".repeat(129);
    // Refused before the request.
    let refused = || Some(("kimi".to_owned(), true));
    assert_eq!(
        [kimi("a.b").await, kimi("1abc").await, kimi(&long).await],
        [refused(), refused(), refused()]
    );
    // Behind another URL the server decides: a dotted name goes on.
    let (body, answered) = sent(kimi_k3, declaring("a.b", hello())).await;
    assert_eq!(
        (body["tools"][0]["function"]["name"].clone(), answered),
        (json!("a.b"), Ok(()))
    );
    let deepseek = DeepSeek::new(DeepSeekConfig::new(KEY, "deepseek-v4-pro")).unwrap();
    let refused = deepseek.generate(&declaring("files.read", hello())).await;
    assert_eq!(refused.unwrap_err().provider, "deepseek");
}

/// Kimi's provider for K3 on a server's URL.
fn kimi_k3(url: &str) -> Kimi {
    kimi("kimi-k3")(url)
}

/// A whole completion carrying `usage`.
fn used(usage: &Value) -> Reply {
    let body = json!({
        "id": "chatcmpl-1",
        "object": "chat.completion",
        "choices": [{ "index": 0, "message": { "role": "assistant", "content": "Hi" },
            "finish_reason": "stop" }],
        "usage": usage,
    });
    json_reply(200, body.to_string())
}

// Python: tests/test_providers/test_deepseek.py::TestUsage::test_cache_hits_are_not_billed_as_input
/// DeepSeek's cache hits are read through its provider, answered whole and
/// streamed.
#[tokio::test]
async fn deepseek_s_cache_hits_are_not_billed_as_input() {
    let usage = json!({ "prompt_tokens": 1000, "completion_tokens": 50,
        "prompt_cache_hit_tokens": 960, "prompt_cache_miss_tokens": 40 });
    let fake = Fake::serving(vec![used(&usage)]).await;
    let whole = deepseek(&url_of(&fake)).generate(&hello()).await;
    let whole = whole.unwrap().usage;
    let chunks = [
        chunk(&json!({ "content": "Hi" })),
        json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }] }),
        json!({ "choices": [], "usage": usage }),
    ];
    let streamed_fake = Fake::serving(vec![events(&chunks)]).await;
    let streaming = deepseek(&url_of(&streamed_fake));
    let context = hello();
    let (read, failure) = drained(streaming.generate_structured_stream(&context)).await;
    assert_eq!(failure, None);
    let Some(StreamEvent::Done(done)) = read.into_iter().next_back() else {
        panic!("the stream ends with its done event");
    };
    let expected =
        json!({ "input_tokens": 40, "output_tokens": 50, "cache_read_input_tokens": 960 });
    assert_eq!(Value::Object(whole), expected);
    assert_eq!(Value::Object(done.usage), expected);
}

// Python: tests/test_providers/test_deepseek.py::TestDeepSeekThinking::test_config_extra_body_survives_thinking
#[tokio::test]
async fn a_configured_extra_body_survives_and_the_turn_s_thinking_wins() {
    let mut extra = roomkit::model::Metadata::new();
    extra.insert("top_k".to_owned(), json!(5));
    extra.insert("thinking".to_owned(), json!({ "type": "enabled" }));
    let configured = |url: &str| {
        DeepSeek::new(DeepSeekConfig {
            base_url: url.to_owned(),
            extra_body: Some(extra),
            ..DeepSeekConfig::new(KEY, "deepseek-v4-pro")
        })
        .unwrap()
    };
    let off = AiContext {
        enable_thinking: Some(false),
        ..hello()
    };
    let (body, answered) = sent(configured, off).await;
    assert_eq!(answered, Ok(()));
    assert_eq!(
        members(&body, &["top_k", "thinking"]),
        json!({ "top_k": 5, "thinking": { "type": "disabled" } })
    );
}

// Python: tests/test_providers/test_minimax_kimi.py::test_the_temperature_goes_where_the_server_takes_it
#[tokio::test]
async fn a_temperature_goes_where_the_config_says_the_server_takes_it() {
    let kimi = |url: &str| {
        Kimi::new(KimiConfig {
            base_url: url.to_owned(),
            supports_custom_temperature: true,
            ..KimiConfig::new(KEY, "kimi-k3")
        })
        .unwrap()
    };
    let minimax = |url: &str| {
        MiniMax::new(MiniMaxConfig {
            base_url: url.to_owned(),
            supports_custom_temperature: true,
            ..MiniMaxConfig::new(KEY, "MiniMax-M3")
        })
        .unwrap()
    };
    let (kimi, _) = sent(kimi, hello()).await;
    let (minimax, _) = sent(minimax, hello()).await;
    assert_eq!(
        (kimi["temperature"].clone(), minimax["temperature"].clone()),
        (json!(0.2), json!(0.2))
    );
}

// Python: tests/test_providers/test_deepseek.py::TestDeepSeekProvider::test_sdk_error_wrapped_with_deepseek_provider
#[tokio::test]
async fn a_vendor_s_failure_names_the_vendor() {
    let answer = json_reply(429, r#"{"error": {"message": "slow down"}}"#);
    let fake = Fake::serving(vec![answer]).await;
    let failed = deepseek(&url_of(&fake)).generate(&hello()).await;
    let failed = failed.unwrap_err();
    assert_eq!(
        (
            failed.provider.as_str(),
            failed.retryable,
            failed.status_code
        ),
        ("deepseek", true, Some(429))
    );
}

// Python: OpenAIAIProvider._listing, a vendor's catalogue backfilling its listing
#[tokio::test]
async fn a_vendor_s_listing_reads_its_own_catalogue() {
    let page = json!({ "object": "list", "data": [{ "id": "kimi-k3" }, { "id": "other" }] });
    let fake = Fake::serving(vec![json_reply(200, page.to_string())]).await;
    let listed = kimi_k3(&url_of(&fake)).list_models().await.unwrap();
    let windows: Vec<(String, Option<u64>)> = listed
        .into_iter()
        .map(|model| (model.id, model.context_window))
        .collect();
    assert_eq!(
        windows,
        [
            ("kimi-k3".to_owned(), Some(1_048_576)),
            ("other".to_owned(), None)
        ]
    );
}

// Python: DeepSeekAIProvider inherits OpenAIAIProvider.supports_vision
#[test]
fn deepseek_reads_a_model_it_does_not_name_as_openai_does() {
    let deepseek = |model: &str| {
        let provider = DeepSeek::new(DeepSeekConfig {
            base_url: "http://gateway:8000/v1".to_owned(),
            ..DeepSeekConfig::new(KEY, model)
        });
        provider.unwrap().supports_vision()
    };
    assert_eq!(
        [
            deepseek("deepseek/deepseek-v4-pro"),
            deepseek("gpt-4o-mini"),
            deepseek("deepseek-v4-pro"),
        ],
        [false, true, false]
    );
}

// Python: tests/test_providers/test_minimax_kimi.py::test_a_response_schema_is_taken_where_the_vendor_documents_it
// and tests/test_providers/test_deepseek.py::TestResponseSchemaDefault::test_off_unless_the_config_turns_it_on
#[test]
fn a_response_schema_is_taken_where_the_vendor_documents_it() {
    let minimax = |stated: Option<bool>| {
        let config = MiniMaxConfig {
            supports_response_schema: stated,
            ..MiniMaxConfig::new(KEY, "MiniMax-M3")
        };
        MiniMax::new(config).unwrap().supports_response_schema()
    };
    let kimi = |stated: Option<bool>| {
        let config = KimiConfig {
            supports_response_schema: stated,
            ..KimiConfig::new(KEY, "kimi-k3")
        };
        Kimi::new(config).unwrap().supports_response_schema()
    };
    let deepseek = |stated: Option<bool>| {
        let config = DeepSeekConfig {
            supports_response_schema: stated,
            ..DeepSeekConfig::new(KEY, "deepseek-v4-flash")
        };
        DeepSeek::new(config).unwrap().supports_response_schema()
    };
    assert_eq!(
        [
            (minimax(None), minimax(Some(true))),
            (kimi(None), kimi(Some(false))),
            (deepseek(None), deepseek(Some(true))),
        ],
        [(false, true), (true, false), (false, true)]
    );
    // Beside function tools a schema is taken on OpenAI's own endpoint
    // alone, as Python's `supports_response_schema_with_tools` reads the
    // base URL: not on a vendor's own, which takes a schema.
    let kimi = Kimi::new(KimiConfig::new(KEY, "kimi-k3")).unwrap();
    assert!(!kimi.supports_response_schema_with_tools());
}
