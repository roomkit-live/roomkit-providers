# Changelog

User-visible changes. Entries under **Unreleased** describe the source tree;
nothing has been released yet.

## Unreleased

### Added

- `roomkit-providers-core`: the request a provider is given, `AiRequest`,
  with its messages (`AiMessage`, `AiRole`, `AiMessageContent`) and their
  parts: text, image, reasoning, tool call and tool result.
- `roomkit-providers-core`: a tool as the model is told of it, `AiTool`, what
  a call answered, `ToolResult`, how it ended, `ToolCallOutcome`, and
  `ValueError` for a value the contract refuses.
- `roomkit-providers-core`: what a generation answers, `AiResponse`, with
  its calls (`AiToolCall`, `ServedCall`), the events of a structured stream
  (`StreamEvent`), and the reading of a call's id and arguments every
  provider applies (`CallIds`, `tool_arguments`, `is_truncation`).
- `roomkit-providers-core`: a provider's call that failed, `ProviderError`,
  with `is_context_overflow_message`.
- `roomkit-providers-core`: the model catalogue, `ModelInfo` and
  `ModelPricing` with `cost_for`, `Date`, and the speech tags of a listing.
- `roomkit-providers-core`: the readings of an image part a provider needs,
  `image_part_payload`, `image_part_base64` and `image_part_uri`.
- `roomkit-providers-core`: what a provider implements, the `AiProvider`
  trait, with `DynAiProvider`, the `ProviderStream` and `TextStream` of a
  generation, and where a host leaves the credential of one request
  (`API_KEY_METADATA_KEY`).
- `roomkit-providers-core`: the response schema, `check_portable_schema`,
  `schema_mismatch` and `ResponseSchemaError`; a request read from JSON
  refuses a schema outside the portable subset.
- `roomkit-providers-core`: the replay of a round, `round_parts`,
  `RoundTranscript` and `ThinkingBlocks`, the rules on a call's arguments
  (`unreadable_arguments`, `call_partial`, `call_cut`, `call_garbled`) and
  `declared_parameters`.
- `roomkit-providers-core`: `MockAiProvider`, a provider answering from a
  list, for tests and examples.
- Each of these types that is a model in RoomKit's Python implementation
  reads and writes the JSON that model gives with `model_dump(mode="json")`,
  checked against examples written by RoomKit's Python. A number is read to
  the float Python wrote, a number of 16 or 17 digits included. As in
  Python, a request's JSON form never carries the credential of one request:
  it is written masked.
- A model is read from a JSON object only, and a name or a `type` from a
  JSON string only, as Python reads them. A refusal that names a value quotes
  it as Python's `repr` does. The crate's documentation lists where input is
  stricter than Python's and what the JSON parser reads differently.
- `roomkit-providers-core` is licensed `MIT AND Unicode-3.0`: its table of
  the characters Python's `repr` writes as they are is derived from Unicode
  data, and the crate ships that data's license, `LICENSE-UNICODE`, beside
  its own.
- `roomkit-providers-http`: `validate_public_url`, which checks that a URL
  names a public destination before a transport is given it, and refuses a
  loopback, private, reserved or multicast address however it is written or
  resolved. It decides as `validate_public_url` of RoomKit's Python
  implementation, in the same words, on a table of 370 URLs and addresses;
  its README lists where it differs.
