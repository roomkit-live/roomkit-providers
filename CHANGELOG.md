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
- Each of these types reads and writes the JSON its model in RoomKit's Python
  implementation gives with `model_dump(mode="json")`, checked against
  examples written by RoomKit's Python. As in Python, a request's JSON form
  never carries the credential of one request: it is written masked.
