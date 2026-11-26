# Managed llama.cpp

`LlamaCpp` runs a GGUF model locally. Pass a file path or a Hugging Face
reference such as `unsloth/Qwen3-4B-Instruct-2507-GGUF:Q4_K_M`. The first
call installs the pinned llama.cpp release for this machine, verifies its
SHA-256 and starts a server on loopback. A reference lets llama-server
download and cache the model. A local file and a cached build work offline.
Set `binary` to use a particular executable, including a build required by
a model's architecture. A bare executable name is resolved on PATH; use an explicit
path to run an executable from another directory. The server must support the model and its chat template.

```rust,no_run
use roomkit_ai::AiProvider;
use roomkit_providers_llamacpp::{LlamaCpp, LlamaCppConfig};

# async fn run() -> Result<(), roomkit_ai::ProviderError> {
let ai = LlamaCpp::new(LlamaCppConfig::new("/models/local.gguf"))?;
ai.start().await?;
let models = ai.list_models().await?;
ai.close().await;
# Ok(()) }
```

Clones share one child. Closing it releases its model; a later request can
start it again. Inference, model listing and streamed tool calls use Chat
Completions. Size the context for messages, tool declarations and results.
The local model's reasoning switch and effort go to its chat template.

Run the example with a local file, optionally followed by a server executable:

```sh
cargo run -p roomkit-providers-llamacpp --example managed_gguf -- /models/local.gguf
```
