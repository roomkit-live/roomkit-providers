# roomkit-providers-core

Runtime-independent contracts for model providers: what a provider is asked,
what it answers, the events of a streamed answer, typed failures and model
catalogues.

The crate has no I/O and no async runtime, and depends on neither Python nor
RoomKit. Provider implementations own their HTTP; hosts own their
conversations.

**Early development.** The crate defines today:

- the request a provider is given, `AiRequest`, with its messages
  (`AiMessage`) and their parts: text, image, reasoning, tool call and tool
  result;
- a tool as the model is told of it, `AiTool`, what a call answered,
  `ToolResult`, and how it ended, `ToolCallOutcome`;
- `ValueError`, the error for a value the contract refuses.

The response, the events of a streamed answer, the typed failures and the
model catalogues are not defined yet. See the
[repository](https://github.com/roomkit-live/roomkit-providers) for the state
of the project.

## Names and JSON forms

A type that RoomKit's Python API names keeps that name in Rust's casing:
Python's `AIMessage` is `AiMessage`. The request is `AiRequest`: the fields a
provider reads of Python's `AIContext`.

Every type reads and writes the JSON its Python model gives with
`model_dump(mode="json")`:

```rust
use roomkit_providers_core::{AiMessage, AiRole};

let message = AiMessage::new(AiRole::User, "Where is my order?");
assert_eq!(
    serde_json::to_string(&message)?,
    r#"{"role":"user","content":"Where is my order?","metadata":{}}"#
);
```

## License

MIT
