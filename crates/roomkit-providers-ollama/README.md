# roomkit-providers-ollama

The Ollama model provider of RoomKit Providers, on Ollama's native API.

- `Ollama`, a provider for the models an Ollama server holds: Python's
  `OllamaAIProvider`. It answers a request whole (`generate`), as a stream
  of events (`generate_structured_stream`) or of text (`generate_stream`),
  and lists the models the server holds, each with the capabilities it
  answers of them (`list_models`);
- `OllamaConfig`, what the provider is configured with. It is the
  `OllamaConfig` of RoomKit's Python implementation: the same names, the
  same defaults, and the JSON it gives with `model_dump(mode="json")`, read
  and written. Where a Python member holds one of two kinds, Rust names
  them: `think` is a `Think`, a switch or a `ThinkEffort`, and `keep_alive`
  a `KeepAlive`, a duration or a count of seconds;
- the wire cases the provider is held to
  (`tests/fixtures/python_ollama_cases.json`): what RoomKit's Python
  provider sends and reads, case by case, captured with
  `scripts/ollama_cases.py`. Every case is replayed against the provider
  through a local fake of the server.

```rust
use roomkit_providers_ollama::{Ollama, OllamaConfig, Think, ThinkEffort};

fn provider() -> Result<Ollama, roomkit_ai::ProviderError> {
    Ollama::new(OllamaConfig {
        model: "gpt-oss:20b".to_owned(),
        think: Some(Think::Effort(ThinkEffort::High)),
        ..OllamaConfig::default()
    })
}
```

## What it sends

- A whole answer is asked whole (`/api/chat`, `stream: false`), a stream
  as JSON lines. A request is sent once: the ollama SDK RoomKit's provider
  runs on tries none again.
- The host is read as the SDK reads it: `http` and port 11434 where no
  scheme is written, the scheme's port where one is, the user information
  left out, a path kept.
- The body is written as the SDK writes it: without tools, the SDK's
  order, the tools an empty list and a count of seconds to keep the model
  loaded written with a fraction; with tools, the order RoomKit writes it
  in, each tool's schema whole. A tool's results go as `tool` messages
  naming the tool, their images on a user message after them.
- A turn's reasoning goes as `think`: off where the turn states off; the
  level nearest the turn's effort for a model configured with a level, the
  one sign that it takes levels; else on, or the model's default.
- An image goes as base64: a data URI's payload; a path that names a file,
  read from the disk (as Python reads it); any other text Python's lenient
  base64 reader takes, as it is. A missing picture, or a text no base64,
  fails before any request.
- The headers the configuration names, the SDK's own over them, the key as
  a bearer token over a configured authorization; where the configuration
  states none, the key `OLLAMA_API_KEY` holds when the provider is built.

## What it reads

- Each answer as the SDK reads it: one without its message, a message
  without its role, a call without its name or with arguments that are no
  object, fails; so does a line of a stream that is no JSON object, a
  blank one included. A line that names an `error` fails the stream,
  retried as a lost connection.
- Text and thinking as they come; the calls whole, each given an id unique
  in its answer, handed on after the text; the end in the contract's words,
  `load` and `unload` as none; the counts the end reports.
- The listing: `/api/tags`, then `/api/show` for each model, one after the
  other; a model the server shows nothing the SDK reads of has no
  capabilities.

## Not covered

Embeddings, `/api/generate` and the pulling of models stay RoomKit's
Python's for now.

One case is named: a listing that fails raises the SDK's own error in
Python, which no provider reproduces; this one fails with a
`ProviderError`.
