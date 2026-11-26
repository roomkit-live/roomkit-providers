//! Load a GGUF or a Hugging Face reference; without arguments, exercise a local stand-in.
#[path = "../tests/support/child.rs"]
mod child;

use roomkit_ai::{AiContext, AiMessage, AiProvider, AiRole};
use roomkit_providers_llamacpp::{LlamaCpp, LlamaCppConfig};
use std::{
    env,
    error::Error,
    io::{self, Write},
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    if let Some(port) = child::argument("--port") {
        return child::serve(port.parse()?).await;
    }
    let model = env::args().nth(1);
    let offline = model.is_none();
    let mut config = LlamaCppConfig::new(model.unwrap_or_else(|| "example/model:Q4_K_M".into()));
    config.binary = if offline {
        Some(env::current_exe()?)
    } else {
        env::args_os().nth(2).map(Into::into)
    };
    let provider = LlamaCpp::new(config)?;
    let request = AiContext {
        messages: vec![AiMessage::new(
            AiRole::User,
            "What is 2 + 2? Answer in one sentence.",
        )],
        ..Default::default()
    };
    let response = provider.generate(&request).await;
    provider.close().await;
    let text = response?.content;
    if text.is_empty() || (offline && text != "done") {
        return Err("unexpected model response".into());
    }
    writeln!(io::stdout().lock(), "{text}")?;
    Ok(())
}
