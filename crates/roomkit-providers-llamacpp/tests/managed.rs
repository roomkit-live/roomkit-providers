//! Managed process lifecycle and parity with the reference provider's recorded requests.
#[path = "support/child.rs"]
mod child;

use futures_util::TryStreamExt;
use roomkit_ai::{AiContext, AiMessage, AiProvider, AiRole, AiTool, StreamEvent};
use roomkit_providers_llamacpp::{LlamaCpp, LlamaCppConfig};
use serde_json::{Value, json};
use std::{env, error::Error, fs, net::TcpListener, path::Path};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    if let Some(port) = child::argument("--port") {
        return child::serve(port.parse()?).await;
    }
    let dir = tempfile::tempdir()?;
    let fixture: Value = serde_json::from_str(include_str!("fixtures/python.json"))?;
    requests(&fixture, dir.path()).await?;
    reasoning_budgets(dir.path()).await?;
    failures(dir.path()).await?;
    cancel_loading(dir.path()).await?;
    Ok(())
}

async fn requests(fixture: &Value, dir: &Path) -> Result<()> {
    let config = LlamaCppConfig::new("example/model:Q4_K_M");
    assert_eq!(serde_json::to_value(&config)?, fixture["defaults"]);
    let record = dir.join("requests");
    let mut config = config;
    config.binary = Some(env::current_exe()?);
    config.enable_thinking = Some(false);
    config.extra_args = vec!["--record".into(), record.display().to_string()];
    let provider = LlamaCpp::new(config)?;
    assert_eq!(provider.name(), "llamacpp");
    assert_eq!(
        serde_json::to_value(LlamaCpp::available_models())?,
        fixture["catalogue"]
    );
    assert!(provider.catalog_entry().is_none());
    let (a, b) = tokio::join!(provider.start(), provider.start());
    a?;
    b?;
    assert_eq!(
        provider
            .list_models()
            .await?
            .first()
            .ok_or("missing model")?
            .id,
        "local-model"
    );
    let mut ask = AiContext {
        messages: vec![AiMessage::new(AiRole::User, "Look up the record.")],
        tools: vec![AiTool {
            parameters: json!({"type":"object","properties":{}}),
            ..AiTool::new("lookup", "Read a record")?
        }],
        ..Default::default()
    };
    for effort in [None, Some("high"), Some("none")] {
        ask.reasoning_effort = effort.map(str::to_owned);
        let reply = provider.generate(&ask).await?;
        assert_eq!(
            reply.tool_calls.first().ok_or("missing call")?.name,
            "lookup"
        );
    }
    let events: Vec<_> = provider
        .generate_structured_stream(&ask)
        .try_collect()
        .await?;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamEvent::ToolCall(_)))
    );
    let log = fs::read_to_string(record)?;
    let mut rows = log.lines().map(serde_json::from_str::<Value>);
    let mut arguments = rows.next().ok_or("no launch")??;
    let port = provider
        .base_url()
        .trim_end_matches("/v1")
        .rsplit(':')
        .next()
        .ok_or("no port")?
        .parse::<u16>()?;
    let expected = fixture["launch"].as_array().ok_or("no arguments")?;
    let args = arguments.as_array_mut().ok_or("bad launch")?;
    assert_eq!(args.get(5), Some(&port.to_string().into()));
    *args.get_mut(5).ok_or("missing port")? = "18080".into();
    assert_eq!(args.get(..9), expected.get(1..));
    for expected in fixture["requests"].as_array().ok_or("no requests")? {
        assert_eq!(&rows.next().ok_or("missing request")??, expected);
    }
    assert_eq!(rows.count(), 1);
    provider.close().await;
    let address = ("127.0.0.1", port);
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    provider.start().await?;
    provider.close().await;
    Ok(())
}

async fn reasoning_budgets(dir: &Path) -> Result<()> {
    let record = dir.join("budgets");
    let mut config = LlamaCppConfig::new("example/model");
    config.binary = Some(env::current_exe()?);
    config.extra_args = vec!["--record".into(), record.display().to_string()];
    let provider = LlamaCpp::new(config)?;
    let budgets = [Some(1024), Some(256), Some(0), Some(-1), None];
    for thinking_budget in budgets {
        let request = AiContext {
            messages: vec![AiMessage::new(AiRole::User, "Hello")],
            max_tokens: Some(4096),
            thinking_budget,
            ..Default::default()
        };
        provider
            .generate_structured_stream(&request)
            .try_collect::<Vec<_>>()
            .await?;
    }
    provider.close().await;
    let log = fs::read_to_string(record)?;
    let mut lines = log.lines().skip(1);
    for budget in budgets {
        let body: Value = serde_json::from_str(lines.next().ok_or("missing request")?)?;
        assert_eq!(body.get("max_tokens"), Some(&json!(4096)));
        assert_eq!(
            body.get("reasoning_budget_tokens").and_then(Value::as_i64),
            budget.map(|n| n.max(0))
        );
        assert_eq!(
            body.pointer("/chat_template_kwargs/enable_thinking")
                .and_then(Value::as_bool),
            budget.map(|n| n > 0)
        );
    }
    assert!(lines.next().is_none());
    Ok(())
}

async fn failures(dir: &Path) -> Result<()> {
    let model = dir.join("local.gguf");
    let mut config = LlamaCppConfig::new(model.display().to_string());
    config.binary = Some(env::current_exe()?);
    let provider = LlamaCpp::new(config.clone())?;
    assert!(provider.start().await.is_err());
    fs::write(&model, b"test")?;
    let busy = TcpListener::bind(("127.0.0.1", 0))?;
    config.port = Some(busy.local_addr()?.port());
    let provider = LlamaCpp::new(config.clone())?;
    assert!(provider.start().await.is_err());
    drop(busy);
    config.extra_args.push("--fail-load".into());
    let provider = LlamaCpp::new(config)?;
    let failed = provider
        .start()
        .await
        .err()
        .ok_or("fake unexpectedly became ready")?;
    assert_eq!(failed.provider, "llamacpp");
    assert!(failed.message.contains("exited"));
    assert!(failed.message.contains("failed to load the model"));
    Ok(())
}

async fn cancel_loading(dir: &Path) -> Result<()> {
    let signal = dir.join("loading");
    let mut config = LlamaCppConfig::new("example/local");
    config.binary = Some(env::current_exe()?);
    config.extra_args = vec![
        "--wait-for-cancellation".into(),
        signal.display().to_string(),
    ];
    let provider = LlamaCpp::new(config)?;
    let endpoint = provider.base_url();
    let addr = endpoint
        .trim_start_matches("http://")
        .trim_end_matches("/v1");
    let other = provider.clone();
    let starting = tokio::spawn(async move { other.start().await });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !signal.exists() {
            tokio::task::yield_now().await;
        }
        starting.abort();
        assert!(starting.await.is_err_and(|err| err.is_cancelled()));
        while tokio::net::TcpStream::connect(addr).await.is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    provider.close().await;
    Ok(())
}
