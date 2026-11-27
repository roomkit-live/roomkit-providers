# roomkit-providers-mistral

The Mistral model provider of RoomKit Providers, on Mistral's chat
completions.

- `Mistral`, a provider for Mistral's models: Python's `MistralAIProvider`.
  It answers a request whole (`generate`), as a stream of events
  (`generate_structured_stream`) or of text (`generate_stream`), and lists
  the models its endpoint serves (`list_models`), on Mistral's endpoint or
  behind a `server_url`;
- `MistralConfig`, what the provider is configured with. It is the
  `MistralConfig` of RoomKit's Python implementation: the same names, the
  same defaults, and the JSON it gives with `model_dump(mode="json")`, read
  and written;
- the wire cases the provider is held to
  (`tests/fixtures/python_mistral_cases.json`): what RoomKit's Python
  provider sends and reads, case by case, captured with
  `scripts/mistral_cases.py`. Every case is replayed against the provider
  through a local fake of the endpoint.

```rust
use roomkit_providers_mistral::{Mistral, MistralConfig};

fn provider() -> Result<Mistral, roomkit_ai::ProviderError> {
    Mistral::new(MistralConfig {
        reasoning_effort: Some("high".to_owned()),
        ..MistralConfig::new("placeholder-not-a-key")
    })
}
```

## What it sends

- Every generation is asked as a stream, `/v1/chat/completions`, a whole
  answer folded from it, the key as a bearer token. A request is sent once,
  and waits up to 300 seconds for each step: the mistralai SDK RoomKit's
  provider runs on tries none again, and RoomKit sets no timeout of its
  own.
- The body is written as the SDK writes it: its members in the SDK's
  order, each message as the SDK's model of its role writes it back, an
  assistant message with `prefix: false` and each of its calls with its
  `type` and its `index`. The conversation is RoomKit's chat rendering,
  each tool message naming its tool.
- A turn's reasoning goes as `reasoning_effort`: the turn's effort over the
  configured one; `none` for a turn that states off, the effort or `high`
  for one that states on; nothing where nothing is stated, the model
  deciding.
- A response schema goes strict, as `response_format`; one asked beside
  tools is refused before any request. On Mistral's own endpoint, a tool
  name Mistral refuses (a colon, say) fails before any request; behind
  another server URL the server decides.

## What it reads

- Each chunk as the SDK reads it: one that lacks what the SDK requires
  (an id, a model, choices, a choice's index, delta or finish reason, a
  call's function name or arguments) fails the stream; a content part the
  SDK does not read as its type is left out.
- Text, reasoning in `<think>` tags or in typed parts of reasoning, as it
  comes; the calls folded across the chunks, an id the SDK writes `"null"`
  read as none; the finish in the contract's words, `model_length` as a
  cut answer.
- Usage as the contract's counters, the cached prompt tokens apart from
  the fresh input.
- A 2xx answer that says it is no stream of events fails, as the SDK reads
  none from it.

## Not covered

The SDK's Azure and Google Cloud clients, embeddings and OCR stay RoomKit's
Python's for now.

Two cases are named. A listing that fails raises the SDK's own error in
Python, which no provider reproduces; this one fails with a
`ProviderError`. An empty key goes in its header as it is written, as
Anthropic's provider sends a request's key; Python's HTTP client refuses
that header on a wire.
