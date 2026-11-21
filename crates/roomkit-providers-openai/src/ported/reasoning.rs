//! The reasoning effort a request carries, against what `OpenAI`'s
//! catalogue says of its model and what the configuration says of a server
//! behind a base URL; and the models the endpoint refuses a turn.

use roomkit_providers_llm::{AiProvider, AiRequest};
use serde_json::{Value, json};

use super::support::{
    body_of, body_sent, elsewhere, hello, hello_with_a_tool, official, pointed, refused, saying,
    tool,
};
use super::warnings::Warnings;
use crate::OpenAiConfig;
use crate::replay::drained;
use crate::replay::fake::Fake;

/// The member of the configuration the one warning names.
const FLAG: &str = "supports_reasoning_effort_with_tools";

/// The configuration for `model` on `OpenAI`'s own endpoint.
fn on_openai(model: &str, effort: Option<&str>, stated: Option<bool>) -> OpenAiConfig {
    OpenAiConfig {
        reasoning_effort: effort.map(str::to_owned),
        supports_reasoning_effort_with_tools: stated,
        ..official(model)
    }
}

/// The configuration for a server behind a base URL, whose model no
/// catalogue knows.
fn behind_base_url(effort: Option<&str>, stated: Option<bool>) -> OpenAiConfig {
    OpenAiConfig {
        reasoning_effort: effort.map(str::to_owned),
        supports_reasoning_effort_with_tools: stated,
        ..elsewhere("local-model")
    }
}

/// How a turn states its reasoning: its own effort, and whether its switch
/// states off.
#[derive(Clone, Copy)]
struct Turn {
    effort: Option<&'static str>,
    off: bool,
}

/// The `reasoning_effort` the request of `turn` carries, with a tool when
/// `tools`; null when it carries none.
async fn effort(config: OpenAiConfig, turn: Turn, tools: bool) -> Value {
    let request = AiRequest {
        reasoning_effort: turn.effort.map(str::to_owned),
        enable_thinking: turn.off.then_some(false),
        tools: if tools {
            vec![tool("lookup")]
        } else {
            Vec::new()
        },
        ..hello()
    };
    body_sent(config, &request).await["reasoning_effort"].clone()
}

/// A turn that states nothing of its reasoning.
const UNSAID: Turn = Turn {
    effort: None,
    off: false,
};

/// A turn whose own effort is `effort`.
fn stating(effort: &'static str) -> Turn {
    Turn {
        effort: Some(effort),
        off: false,
    }
}

// Source: src/reasoning/tests.rs::a_tagged_model_follows_the_catalogue_whatever_the_config_states
#[tokio::test]
async fn a_tagged_model_follows_the_catalogue_whatever_the_config_states() {
    let (warnings, _capturing) = Warnings::capture();
    for stated in [None, Some(true), Some(false)] {
        let keeps = effort(on_openai("gpt-5-mini", Some("low"), stated), UNSAID, true).await;
        let unset = effort(on_openai("gpt-6-luna", None, stated), UNSAID, true).await;
        let locked = effort(on_openai("gpt-6-luna", Some("low"), stated), UNSAID, true).await;
        assert_eq!(keeps, json!("low"), "{stated:?}");
        assert_eq!(unset, json!("none"), "{stated:?}");
        assert_eq!(locked, json!("none"), "{stated:?}");
    }
    assert_eq!(warnings.containing(FLAG), Vec::<String>::new());
}

// Source: src/reasoning/tests.rs::the_config_decides_for_a_model_the_catalogue_does_not_tag
#[tokio::test]
async fn the_config_decides_for_a_model_the_catalogue_does_not_tag() {
    for (stated, expected) in [(true, json!("low")), (false, Value::Null)] {
        let config = on_openai("gpt-7-preview", Some("low"), Some(stated));
        assert_eq!(effort(config, UNSAID, true).await, expected, "{stated}");
    }
}

// Source: src/reasoning/tests.rs::a_model_the_catalogue_does_not_tag_is_left_out_with_a_warning
#[tokio::test]
async fn a_model_the_catalogue_does_not_tag_is_left_out_with_a_warning() {
    let (warnings, _capturing) = Warnings::capture();
    let config = on_openai("gpt-7-preview", Some("low"), None);
    assert_eq!(effort(config, UNSAID, true).await, Value::Null);
    assert_eq!(warnings.containing(FLAG).len(), 1);
}

// Source: src/reasoning/tests.rs::it_is_left_out_with_one_warning_naming_the_field
#[tokio::test]
async fn it_is_left_out_with_one_warning_naming_the_field() {
    let (warnings, _capturing) = Warnings::capture();
    let fake = Fake::serving(vec![saying("ok"), saying("ok")]).await;
    let provider = pointed(behind_base_url(Some("low"), None), &fake);
    for at in 0..2 {
        provider.generate(&hello_with_a_tool()).await.unwrap();
        assert_eq!(body_of(&fake, at)["reasoning_effort"], Value::Null);
    }
    let warned = warnings.containing(FLAG);
    assert_eq!(warned.len(), 1, "{warned:?}");
    // The effort and the model are the warning's own fields.
    assert!(
        warned[0].contains("effort=low ") && warned[0].contains("model=local-model "),
        "{warned:?}"
    );
}

// Source: src/reasoning/tests.rs::nothing_stated_warns_of_nothing
#[tokio::test]
async fn nothing_stated_warns_of_nothing() {
    let (warnings, _capturing) = Warnings::capture();
    let config = behind_base_url(None, None);
    assert_eq!(effort(config, UNSAID, true).await, Value::Null);
    assert_eq!(warnings.containing(FLAG), Vec::<String>::new());
}

// Source: src/reasoning/tests.rs::a_server_said_not_to_take_it_gets_no_warning
#[tokio::test]
async fn a_server_said_not_to_take_it_gets_no_warning() {
    let (warnings, _capturing) = Warnings::capture();
    let config = behind_base_url(Some("low"), Some(false));
    assert_eq!(effort(config, UNSAID, true).await, Value::Null);
    assert_eq!(warnings.containing(FLAG), Vec::<String>::new());
}

// Source: src/reasoning/tests.rs::off_sends_the_model_s_floor
#[tokio::test]
async fn off_sends_the_model_s_floor() {
    let floors = [
        ("gpt-5.1", "none"),
        ("gpt-5.2", "none"),
        ("gpt-5", "minimal"),
        ("gpt-5-nano", "minimal"),
        ("o3", "low"),
    ];
    for (model, floor) in floors {
        for tools in [false, true] {
            // Off stated by the turn over a configured effort.
            let over = on_openai(model, Some("high"), None);
            assert_eq!(
                effort(over, stating("none"), tools).await,
                json!(floor),
                "{model} tools={tools}"
            );
            // Off stated by the configuration alone.
            let configured = on_openai(model, Some("none"), None);
            assert_eq!(
                effort(configured, UNSAID, tools).await,
                json!(floor),
                "{model} tools={tools}, configured"
            );
            // Off stated by the turn's switch, nothing configured.
            let off = Turn {
                effort: None,
                off: true,
            };
            assert_eq!(
                effort(on_openai(model, None, None), off, tools).await,
                json!(floor),
                "{model} tools={tools}, off"
            );
        }
    }
}

// Source: src/reasoning/tests.rs::behind_a_base_url_an_effort_of_none_has_no_floor
#[tokio::test]
async fn behind_a_base_url_an_effort_of_none_has_no_floor() {
    let config = OpenAiConfig {
        reasoning_effort: Some("none".to_owned()),
        ..elsewhere("gpt-5")
    };
    assert_eq!(effort(config, UNSAID, false).await, json!("none"));
}

// Source: src/reasoning/tests.rs::tools_for_a_model_that_takes_none_fail_before_the_request
#[tokio::test]
async fn tools_for_a_model_that_takes_none_fail_before_the_request() {
    for model in ["gpt-6-astra", "gpt-6.1-sol"] {
        let error = refused(official(model), &hello_with_a_tool()).await;
        assert!(
            error.to_string().contains("takes no function tools"),
            "{error}"
        );
        assert!(!error.retryable);
        assert!(error.before_request);
    }
}

// Source: src/reasoning/tests.rs::a_responses_only_model_fails_before_the_request
#[tokio::test]
async fn a_responses_only_model_fails_before_the_request() {
    for model in ["gpt-5.5-pro", "gpt-5.4-pro", "o3-pro"] {
        let error = refused(official(model), &hello()).await;
        assert!(error.to_string().contains("Responses API only"), "{error}");
        assert!(error.before_request);
        // Asked as a stream, the refusal is the stream's one item.
        let fake = Fake::serving(vec![saying("never")]).await;
        let provider = pointed(official(model), &fake);
        let (events, failure) = drained(provider.generate_structured_stream(&hello())).await;
        let error = failure.unwrap();
        assert!(events.is_empty() && error.before_request, "{error}");
        assert!(error.to_string().contains("Responses API only"), "{error}");
        assert!(fake.seen().is_empty(), "nothing is sent");
    }
}

// Source: src/reasoning/tests.rs::a_refused_model_is_named_before_a_refused_tool_name
#[tokio::test]
async fn a_refused_model_is_named_before_a_refused_tool_name() {
    let request = AiRequest {
        tools: vec![tool("ns.lookup")],
        ..hello()
    };
    let error = refused(official("gpt-6-astra"), &request).await;
    assert!(
        error.to_string().contains("takes no function tools"),
        "{error}"
    );
}

// Source: src/reasoning/tests.rs::a_turn_value_outranks_the_configured_one_even_when_falsy
#[tokio::test]
async fn a_turn_value_outranks_the_configured_one_even_when_falsy() {
    let configured = || behind_base_url(Some("high"), None);
    assert_eq!(effort(configured(), stating(""), false).await, json!(""));
    assert_eq!(effort(configured(), UNSAID, false).await, json!("high"));
    assert_eq!(
        effort(configured(), stating("low"), false).await,
        json!("low")
    );
    let unset = behind_base_url(None, None);
    assert_eq!(effort(unset, UNSAID, false).await, Value::Null);
}

// Source: src/reasoning/tests.rs::a_tool_turn_carries_the_effort_as_a_turn_without_does
#[tokio::test]
async fn a_tool_turn_carries_the_effort_as_a_turn_without_does() {
    let config = on_openai("gpt-5-mini", Some("high"), None);
    assert_eq!(effort(config, stating("low"), true).await, json!("low"));
}

// Source: src/reasoning/tests.rs::the_turn_effort_outranks_the_config_there
#[tokio::test]
async fn the_turn_effort_outranks_the_config_there() {
    let config = behind_base_url(Some("high"), Some(true));
    assert_eq!(
        effort(config, stating("minimal"), true).await,
        json!("minimal")
    );
}

// Source: src/reasoning/tests.rs::a_turn_that_states_off_sends_none_where_the_server_takes_it
#[tokio::test]
async fn a_turn_that_states_off_sends_none_where_the_server_takes_it() {
    let config = behind_base_url(Some("low"), Some(true));
    assert_eq!(effort(config, stating("none"), true).await, json!("none"));
}

// Source: tests/agent/reasoning.rs::a_turn_without_tools_is_left_as_it_was
#[tokio::test]
async fn a_turn_without_tools_is_left_as_it_was() {
    let config = behind_base_url(Some("low"), Some(false));
    assert_eq!(effort(config, UNSAID, false).await, json!("low"));
}

// Source: tests/agent/reasoning.rs::the_switch_is_unset_by_default and
// tests/agent/retries.rs::by_default_a_request_is_tried_once (its default)
#[test]
fn the_switch_is_unset_and_a_request_is_tried_once_by_default() {
    let config = official("local-model");
    assert_eq!(config.supports_reasoning_effort_with_tools, None);
    assert_eq!(config.max_retries, 0);
}

// Source: src/reasoning/tests.rs::a_model_that_does_not_reason_is_sent_no_off_value
#[tokio::test]
async fn a_model_that_does_not_reason_is_sent_no_off_value() {
    let off = Turn {
        effort: None,
        off: true,
    };
    let plain = on_openai("gpt-4.1", None, None);
    assert_eq!(effort(plain, off, false).await, Value::Null);
    let proxied = behind_base_url(None, None);
    assert_eq!(effort(proxied, off, false).await, Value::Null);
}
