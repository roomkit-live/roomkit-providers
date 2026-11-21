# Changelog

User-visible changes. Entries under **Unreleased** describe the source tree;
nothing has been released yet.

## Unreleased

### Added

- `roomkit-providers-llm`: the request a provider is given, `AiRequest`,
  with its messages (`AiMessage`, `AiRole`, `AiMessageContent`) and their
  parts: text, image, reasoning, tool call and tool result.
- `roomkit-providers-llm`: a tool as the model is told of it, `AiTool`, what
  a call answered, `ToolResult`, how it ended, `ToolCallOutcome`, and
  `ValueError` for a value the contract refuses.
- `roomkit-providers-llm`: what a generation answers, `AiResponse`, with
  its calls (`AiToolCall`, `ServedCall`), the events of a structured stream
  (`StreamEvent`), and the reading of a call's id and arguments every
  provider applies (`CallIds`, `tool_arguments`, `is_truncation`).
- `roomkit-providers-llm`: a provider's call that failed, `ProviderError`,
  with `is_context_overflow_message`.
- `roomkit-providers-llm`: the model catalogue, `ModelInfo` and
  `ModelPricing` with `cost_for`, `Date`, and the speech tags of a listing.
- `roomkit-providers-llm`: the readings of an image part a provider needs,
  `image_part_payload`, `image_part_base64` and `image_part_uri`.
- `roomkit-providers-llm`: what a provider implements, the `AiProvider`
  trait, with the `label` its failures carry and `DynAiProvider`, which
  holds any provider as an object and answers all a provider does, the
  `ProviderStream` and `TextStream` of a
  generation, and where a host leaves the credential of one request
  (`API_KEY_METADATA_KEY`).
- `roomkit-providers-llm`: the response schema, `check_portable_schema`,
  `schema_mismatch` and `ResponseSchemaError`; a request read from JSON
  refuses a schema outside the portable subset.
- `roomkit-providers-llm`: the replay of a round, `round_parts`,
  `RoundTranscript` and `ThinkingBlocks`, the rules on a call's arguments
  (`unreadable_arguments`, `call_partial`, `call_cut`, `call_garbled`) and
  `declared_parameters`.
- `roomkit-providers-llm`: `MockAiProvider`, a provider answering from a
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
- `roomkit-providers-llm` is licensed `MIT AND Unicode-3.0`: its table of
  the characters Python's `repr` writes as they are is derived from Unicode
  data, and the crate ships that data's license, `LICENSE-UNICODE`, beside
  its own.
- `roomkit-providers-http`: `validate_public_url`, which checks that a URL
  names a public destination before a transport is given it. It refuses a
  host that is this machine by name, an address that is loopback, private,
  reserved or multicast, or otherwise off the public internet, in the forms
  the URL parser of `reqwest` reads, and a name one of whose addresses is,
  as it resolves when the call is made. It decides as `validate_public_url`
  of RoomKit's Python implementation, in the same words, on a table of 406
  URLs and addresses. Its README lists
  where it differs and what it does not guard: a name the client resolves
  again, a client behind a proxy, redirects, and a name when resolution is
  turned off.
- `roomkit-providers-openai`: `OpenAiConfig`, what a provider for OpenAI's
  Chat Completions is configured with: the names, the defaults and the JSON
  form of `OpenAIConfig` in RoomKit's Python implementation, the key written
  masked. Its `Debug` form prints neither the key nor what a base URL holds
  beside its scheme, host and port, as the HTTP client reads them. The
  crate's README lists where it differs from Python's.
- `roomkit-providers-openai`: `OpenAi`, a provider for OpenAI's
  Chat Completions and the servers that speak it behind a base URL. It
  answers a request whole (`generate`), as `OpenAIAIProvider` of RoomKit's
  Python implementation does on the cases captured from it; its README lists
  where it differs and why. A redirect is refused, the key appears in no
  failure, an answer past 4 MiB fails, a 200 whose body is JSON and no
  completion fails, carrying what the body says of it, and nothing is read
  from the environment: no base URL, no organization, no proxy. Only the
  public certificate roots compiled in are trusted. An idle connection is
  kept 5 seconds, and 100 of them for a host at most.
- `roomkit-providers-openai`: the provider's streams,
  `generate_structured_stream` and `generate_stream`. A stream is pulled,
  and dropping it stops the request and closes its connection. It fails at
  a line or an event past 1 MiB and past 256 MiB in all, and one that stalls
  ends at the configuration's timeout. A stream that ends with neither a
  finish reason nor its end mark fails after what came, and hands no whole
  call; a JSON error object in place of a stream fails as the status it
  names; a byte order mark that starts a stream is ignored.
- `roomkit-providers-openai`: `list_models`, the models the endpoint lists,
  each filled from the crate's catalogue where it knows the model.
- `roomkit-providers-openai`: a request the server refuses fails as
  `the provider answered HTTP <status> (<type>, <code>): <message>`, from
  the body's error object alone, or from the text a server words its
  refusal with under `detail`. A body that is no JSON is not echoed. The
  failure says the context overflowed when the body says so, by its code or
  by its words.
- `roomkit-providers-openai`: no failure's text repeats a secret of the
  configuration a server sends back (the key, a default header's value, a
  base URL's user information), as it is or as the failure's own code
  writes it again, nor a control character or a character that shows
  nothing; a secret shorter than 8 characters is not searched for. The
  README says what a failure repeats of an answer, and the bounds of a
  stream's calls.
- `roomkit-providers-openai`: a base URL with user information is sent with
  it as Basic credentials, in place of the key, as Python's client does. A
  base URL for which the client and Python's would not send the same
  request, and a timeout the clock cannot count, are refused when the
  provider is built.
- `roomkit-providers-openai`: `supports_vision` answers as Python's
  provider; the configuration's `vision` states it where the provider would
  not.
- `roomkit-providers-openai`: an example, `local_server`, that asks a
  stand-in server on a local port once whole and once streamed. `make all`
  runs it.
- `roomkit-providers-client`: the HTTP client a provider is built on, which
  `roomkit-providers-openai` is now written on: a request and its tries, the
  bounds of an answer and of a stream, the base URLs refused when a provider
  is built, the one exit of failures and, for a provider's tests, the replay
  of its recorded wire cases. A provider crate holds its wire and nothing
  else.
- `roomkit-providers`, the Python package (CPython 3.12 or newer, Linux
  x86_64, built from the repository), for RoomKit, which it depends on
  (0.117): `roomkit_providers.llm.openai` holds `OpenAI` and `OpenAIConfig`,
  the Rust OpenAI provider behind a native extension. `OpenAI` is a RoomKit
  `AIProvider`, to give an `AIChannel` in place of RoomKit's own: it takes
  an `AIContext`, gives RoomKit's models, raises RoomKit's `ProviderError`
  and `ResponseSchemaError`, and records the time to the first token as
  RoomKit's provider does, under the same name. A context's metadata does
  not cross to the provider, and the credential of one request crosses
  apart. A context the contract refuses, or one with no JSON form, fails
  before it is sent, with RoomKit's `ProviderError` under the provider's
  label; so does a defect of the native code, and a stream that closes
  without its end. A refused configuration leaves no secret in a
  traceback's frames. The calls run on four threads of the package's own,
  whatever their number. A child forked after its parent's first call is
  refused in Python, before it enters the native code. Cancelling a call's
  task, or closing a stream, closes its
  connection. There is no `transport` parameter. The guide
  (`guide/python.md`) says how to switch and what differs.
- `roomkit-providers-http` depends on no HTTP client: it reads a host with
  the `url` crate, the parser `reqwest` reads a URL with.
- `roomkit-providers-http` is licensed `MIT AND Unicode-3.0`: its list of
  the characters that normalize to a delimiter of a URL is derived from
  Unicode data, and the crate ships that data's license, `LICENSE-UNICODE`,
  beside its own.
