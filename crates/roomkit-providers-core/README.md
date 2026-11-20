# roomkit-providers-core

Runtime-independent contracts for model providers: what a provider is asked,
what it answers, the events of a streamed answer, typed failures and model
catalogues.

The crate has no I/O and no async runtime, and depends on neither Python nor
RoomKit. Provider implementations own their HTTP; hosts own their
conversations.

Section numbers in its documentation (`RFC §6.7`) refer to the
[RoomKit RFC](https://github.com/roomkit-live/roomkit-specs).

**Early development.** The crate defines today:

- what a provider implements, the `AiProvider` trait, with the events and
  the text of a generation as streams;
- the request a provider is given, `AiRequest`, with its messages
  (`AiMessage`) and their parts: text, image, reasoning, tool call and tool
  result, and the readings of an image part a provider needs;
- a tool as the model is told of it, `AiTool`, what a call answered,
  `ToolResult`, and how it ended, `ToolCallOutcome`;
- what a generation answers, `AiResponse`, the calls it makes, `AiToolCall`,
  and the events of a structured stream, `StreamEvent`;
- how a round of a turn is replayed, `RoundTranscript` and
  `ThinkingBlocks`, and the reading of a call's arguments every provider
  applies;
- the portable subset of JSON Schema a response schema stays within,
  `check_portable_schema`, and the failure of one, `ResponseSchemaError`;
- what a provider knows of its models, `ModelInfo`, and their list price,
  `ModelPricing`, which prices a response's usage;
- a provider's call that failed, `ProviderError`, and `ValueError`, the
  error for a value the contract refuses;
- a provider answering from a list, `MockAiProvider`, for tests and
  examples.

No provider for a model service is part of this crate. See the
[repository](https://github.com/roomkit-live/roomkit-providers) for the state
of the project.

## Names and JSON forms

A type that RoomKit's Python API names keeps that name in Rust's casing:
Python's `AIMessage` is `AiMessage`. The request is `AiRequest`: the fields a
provider reads of Python's `AIContext`.

Every type that is a model in Python reads and writes the JSON its Python
model gives with `model_dump(mode="json")`:

```rust
use roomkit_providers_core::{AiMessage, AiRole};

let message = AiMessage::new(AiRole::User, "Where is my order?");
assert_eq!(
    serde_json::to_string(&message)?,
    r#"{"role":"user","content":"Where is my order?","metadata":{}}"#
);
```

A model is read from a JSON object only, and a name or a `type` from a JSON
string only. The crate's documentation lists where input is stricter than
Python's, and what the JSON parser reads differently.

## License

`MIT AND Unicode-3.0`.

The crate is under the MIT license (`LICENSE`). Its table of the characters
Python's `repr` writes as they are (`src/python/printable.rs`) is derived
from Unicode data, under the Unicode License V3 (`LICENSE-UNICODE`).
