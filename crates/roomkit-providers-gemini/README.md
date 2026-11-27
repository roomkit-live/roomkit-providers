# roomkit-providers-gemini

The Gemini model provider of RoomKit Providers, on Google's Developer API.

- `Gemini`, a provider for Gemini's models: Python's `GeminiAIProvider`. It
  answers a request whole (`generate`), as a stream of events
  (`generate_structured_stream`) or of text (`generate_stream`), and lists
  the models its endpoint serves (`list_models`), every page of them;
- `GeminiConfig`, what the provider is configured with. It is the
  `GeminiConfig` of RoomKit's Python implementation: the same names, the same
  defaults, and the JSON it gives with `model_dump(mode="json")`, read and
  written;
- the wire cases the provider is held to
  (`tests/fixtures/python_gemini_cases.json`): what RoomKit's Python provider
  sends and reads, case by case, captured with `scripts/gemini_cases.py`.
  Every case is replayed against the provider through a local fake of the
  endpoint.

```rust
use roomkit_providers_gemini::{Gemini, GeminiConfig};

fn provider() -> Result<Gemini, roomkit_ai::ProviderError> {
    Gemini::new(GeminiConfig {
        thinking_level: Some("low".to_owned()),
        ..GeminiConfig::new("placeholder-not-a-key")
    })
}
```

## What it sends

- Every generation is asked as a stream,
  `models/{model}:streamGenerateContent?alt=sse`, a whole answer folded
  from it, the key in `x-goog-api-key`. A request is sent once: the SDK
  RoomKit's provider runs on tries none again.
- A turn's reasoning goes as Gemini's thinking config: the level of the
  model's catalogue nearest the turn's effort, else the configured
  `thinking_level`, else a positive budget, else `-1` (the model sizing it)
  for a turn that states on; for a turn that states off, a budget of `0`,
  or the lowest level on a model that cannot stop reasoning. A model the
  catalogue gives no level is sent no level.
- A round's calls go back with their thought signatures, the round's lent to
  a call Gemini left unsigned. A round of the current turn none of whose
  calls is signed, on a model that checks signatures, goes back as text, its
  results fenced, as the model reads a round another vendor answered.
- A tool's schema is declared as Gemini takes it: a union and a type list
  folded, the implied shape spelled out, an enum of no strings described,
  every key Gemini does not take dropped, `required` kept for the properties
  that survive. A name Gemini refuses (a leading digit) fails before any
  request.
- A history that ends with the model is refused before any request.

## What it reads

- Text and thoughts as they come; the calls folded across the chunks, a
  call re-emitted later folding into the first, two identical calls of one
  chunk staying two; the finish in the contract's words, a refusal under
  `refusal`, a blocked prompt under `prompt_block_reason`.
- Usage as the contract's disjoint counters: the input without what the
  cache held, the thoughts counted as output and told as
  `reasoning_tokens`; the input told once, as the stream starts.

## Not covered

Vertex AI (`GeminiVertexProvider`), images, Gemini Live and the realtime
voice stay RoomKit's Python's for now.

One case is named: a listing that fails raises the SDK's own error in
Python, which no provider reproduces; this one fails with a
`ProviderError`.
