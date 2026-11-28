# Changelog

User-visible changes. Entries under **Unreleased** describe the source tree;
nothing has been released yet.

## Unreleased

### Fixed

- The managed llama.cpp provider sends the per-request reasoning token limit
  to the server. Omitting the limit preserves the server default.
- The SQLite and Postgres stores refuse an event added at an index another
  event holds in its room with `StoreError::EventIndexTaken`, nothing
  written, as RoomKit's Python stores raise `EventIndexTakenError` (RFC
  §8.1); a second add of a stored event stays the database's refusal of
  its id. They pass the core's store contract on roomkit-rs 654b4298.
- Managed llama.cpp and its GPU probe inherit only the environment needed for
  model loading, hardware selection and the local runtime.

### Added

- `OpenAI`'s image provider, in Rust: `OpenAiImage` and `OpenAiImageConfig`
  in the `roomkit-providers-openai` crate, RoomKit's Python
  `OpenAIImageProvider` on the contract's `ImageProvider` (RFC §25):
  generations and edits (multipart), whole or as a stream of previews, its
  catalogue Python's. It is held to 52 cases captured on RoomKit's Python
  provider, 51 replayed as Python and one named. A cancelled generation
  tells its attempt unknown; an answer is read to 512 MiB, an event of a
  stream to 64 MiB. The replay harness learns the image surface, and the
  shared client multipart bodies (a random boundary, a control character
  in a header refused), the headers of an answer, larger bounds for a
  provider that asks them, and whether a request that failed in transport
  had left.
- The image providers in Python: `roomkit_providers.image.openai`
  (`OpenAIImage`, `OpenAIImageConfig`) and `roomkit_providers.image.gemini`
  (`GeminiImage`, `GeminiImageConfig`), each a RoomKit `ImageProvider` over
  the Rust provider, in place of RoomKit's `OpenAIImageProvider` and
  `GeminiImageProvider`, taking RoomKit's configuration as well. Each attempt
  crosses to the host's progress callback, which the provider waits for; a
  cancelled generation tells its attempts in flight unknown. The 116 recorded
  image cases replay through them as Python.
- Gemini's image provider, in Rust: `GeminiImage` and `GeminiImageConfig`
  in the `roomkit-providers-gemini` crate, RoomKit's Python
  `GeminiImageProvider` on the contract's `ImageProvider` (RFC §25): an
  interaction of the Interactions API per image, `n` made at once and each
  settled alone, a size asked as a ratio and a tier, an interaction cut at
  the decode limit continued hop after hop as Python's SDK continues it. It
  is held to 63 cases captured on RoomKit's Python provider, every one
  replayed as Python. The replay harness compares the attempts told of
  attempts made at once each in its own order.
- Gemini on Vertex AI, in Rust and in Python: `GeminiVertex` and
  `GeminiVertexConfig` in the `roomkit-providers-gemini` crate, and
  `roomkit_providers.llm.vertex` (`GeminiVertex`, `GeminiVertexConfig`) as a
  RoomKit `AIProvider`, in place of RoomKit's `GeminiVertexProvider`. It is
  held to the cases captured on RoomKit's Python provider, 39 wire cases, 27
  replayed as Python, 12 named (a token that cannot be had fails before the
  request; an external account is not taken; a failed listing), and to the JSON forms of its configuration. It calls as the
  service account of a key file, or as Application Default Credentials
  (the file GOOGLE_APPLICATION_CREDENTIALS names, gcloud's file, the
  metadata server), a borrowed account too, through RoomKit Auth's
  `google-cloud` identities; external accounts are not taken yet.

- PolarGrid, on the chat completions of its edges, in Rust and in Python:
  the `roomkit-providers-polargrid` crate (`PolarGrid`, `PolarGridConfig`,
  `PolarGridRegion`), and `roomkit_providers.llm.polargrid` (`PolarGrid`,
  `PolarGridConfig`) as a RoomKit `AIProvider`, in place of RoomKit's
  `PolarGridAIProvider`. It is held to the cases captured on RoomKit's
  Python provider, 98 wire cases, each replayed as Python, and to the JSON
  forms of its configuration; it takes RoomKit's own configuration. As
  polargrid-sdk does, it asks the autorouter for an edge where no region is
  pinned, keeps the pick for the whole process, and reads an empty key from
  `POLARGRID_API_KEY`. `available_regions` and `connected_region` are on the
  Rust provider only, for now.

- Rust hosts can supply a renewable OAuth bearer token and a Google Cloud quota project for Gemini model discovery and generation.

- Ollama, on its native API, in Rust and in Python: the
  `roomkit-providers-ollama` crate (`Ollama`, `OllamaConfig`, with `Think`,
  `ThinkEffort` and `KeepAlive` for its two members of two kinds), and
  `roomkit_providers.llm.ollama` (`Ollama`, `OllamaConfig`) as a RoomKit
  `AIProvider`, in place of RoomKit's `OllamaAIProvider`. It is held to the
  cases captured on RoomKit's Python provider, 114 wire cases, each replayed
  as Python but one (a failed listing fails as a failed generation does),
  to the JSON forms of its configuration and to what the ollama SDK makes
  of 40 image texts; it takes RoomKit's own configuration. As Python's, it
  reads an image a path names from the disk, and sends the key
  `OLLAMA_API_KEY` holds where the configuration states no authorization.
- Mistral, on its chat completions, in Rust and in Python: the
  `roomkit-providers-mistral` crate (`Mistral`, `MistralConfig`), and
  `roomkit_providers.llm.mistral` (`Mistral`, `MistralConfig`) as a RoomKit
  `AIProvider`, in place of RoomKit's `MistralAIProvider`. It is held to the
  cases captured on RoomKit's Python provider, 115 wire cases, each replayed
  as Python but two (a failed listing fails as a failed generation does; an
  empty key goes in its header as written, which Python's HTTP client
  refuses), and to the JSON forms of its configuration; it takes RoomKit's
  own configuration.
- Gemini, on Google's Developer API, in Rust and in Python: the
  `roomkit-providers-gemini` crate (`Gemini`, `GeminiConfig`), and
  `roomkit_providers.llm.gemini` (`Gemini`, `GeminiConfig`) as a RoomKit
  `AIProvider`, in place of RoomKit's `GeminiAIProvider`. It is held to the
  cases captured on RoomKit's Python provider, 122 wire cases, each replayed
  as Python but one (a failed listing fails as a failed generation does),
  and to the JSON forms of its configuration; it takes RoomKit's own
  configuration.
- OpenRouter, LiteLLM, Azure and vLLM, in Rust and in Python: `OpenRouter`,
  `LiteLlm`, `AzureAi` and `Vllm` with their configurations in the OpenAI
  crate, and `roomkit_providers.llm.openrouter` (`OpenRouter`,
  `OpenRouterConfig`), `.litellm` (`LiteLLM`, `LiteLLMConfig`), `.azure`
  (`AzureAI`, `AzureAIConfig`) and `.vllm` (`VLLM`, `VLLMConfig`, in place
  of RoomKit's `create_vllm_provider`) as RoomKit `AIProvider`s. Each is
  held to the cases captured on RoomKit's Python provider of the vendor,
  220 wire cases, every one replayed as Python, and to the JSON forms of its
  configuration. LiteLLM's listing dates the proxy's prices in UTC; an
  Azure call with an empty key is refused before any request.
- xAI, Cerebras, Qwen and Meta, in Rust and in Python: `Xai`, `Cerebras`,
  `Qwen` and `Meta` with their configurations in the OpenAI crate, and
  `roomkit_providers.llm.xai` (`XAI`, `XAIConfig`), `.cerebras`
  (`Cerebras`, `CerebrasConfig`), `.qwen` (`Qwen`, `QwenConfig`) and
  `.meta` (`Meta`, `MetaConfig`) as RoomKit `AIProvider`s. Each is held to
  the cases captured on RoomKit's Python provider of the vendor, 202 wire
  cases, every one replayed as Python, and to the JSON forms of its
  configuration; each takes RoomKit's own configuration of the vendor, and
  names itself as RoomKit's provider does.
- DeepSeek, Kimi and MiniMax in Python: `roomkit_providers.llm.deepseek`
  (`DeepSeek`, `DeepSeekConfig`), `roomkit_providers.llm.kimi` (`Kimi`,
  `KimiConfig`) and `roomkit_providers.llm.minimax` (`MiniMax`,
  `MiniMaxConfig`), the Rust providers of the OpenAI crate as RoomKit
  `AIProvider`s. Each takes RoomKit's own configuration of the vendor, and
  names itself as RoomKit's provider does. The three Rust providers are held
  to cases captured on RoomKit's Python providers, as OpenAI's is: 145 wire
  cases, every one replayed as Python, and the JSON forms of their
  configurations, which they now read and write.
- A managed llama.cpp provider for Rust and Python. Load a local GGUF or a
  Hugging Face reference, stream tool calls and release the local model on close.

- `roomkit-providers-sqlite`: RoomKit's conversation store in one SQLite
  file, for a host built on RoomKit's Rust core. `SqliteStore` and its
  options are the crate `roomkit-sqlite` of RoomKit's Rust workspace,
  moved as it was: the file of RoomKit's Python `SQLiteStore`, which
  either implementation reads. The store passes the core's store
  contract (`roomkit::testing::check_conversation_store`). The server
  examples run on it. It needs Rust 1.92, as in RoomKit's core. Rust
  only: RoomKit's Python keeps its own.
- `roomkit-providers-postgres`: RoomKit's conversation store, room lock
  and knowledge source on Postgres, for a host built on RoomKit's Rust
  core. `PostgresStore`, `PostgresAdvisoryLockManager`,
  `PostgresKnowledgeSource` and their options are the crate
  `roomkit-postgres` of RoomKit's Rust workspace, moved as it was: the
  schema of RoomKit's Python `PostgresStore`, byte for byte, the same
  advisory keys, a store both implementations share. The store passes
  the core's store contract (`roomkit::testing::check_conversation_store`).
  An application that takes it repeats the root patch of the Postgres
  driver its README names. It needs Rust 1.92, as in RoomKit's core.
  Rust only: RoomKit's Python keeps its own.
- `roomkit-providers-opentelemetry`: RoomKit's telemetry on OpenTelemetry,
  for a host built on RoomKit's Rust core. `OpenTelemetryProvider` and
  `OpenTelemetryProviderOptions` are the provider that was the core's
  feature `opentelemetry`, with the options and the behaviour it had there:
  the kit's spans and metrics handed to an OpenTelemetry SDK tracer and
  meter provider, a turn of another process in its owner's trace. It starts
  its spans under the current span's context, which the core gives by its
  public `telemetry::get_current_telemetry_ctx`. It needs Rust 1.92, as
  in RoomKit's core. Rust only: RoomKit's
  Python keeps its own.
- `roomkit-providers-redis`: RoomKit's turn backend across processes, on
  Redis, for a host built on RoomKit's Rust core. `RedisTurnBackend` is the
  backend that was in the core, with the constructor and the operations it
  had there; `RedisTurnBroker` and `RedisTurnRegistry` are its two services,
  the ports of the core's `DistributedTurnBackend`, which
  `RedisTurnBackend::broker` and `RedisTurnBackend::registry` give. The
  keys, the scripts and the messages are those of RoomKit's Python backend:
  a Rust process and a Python process on one server serve each other's
  turns. Rust only: RoomKit's Python keeps its own.
- A generation's time to first token is recorded for a host built on
  RoomKit's Rust core, as the metric `roomkit.llm.ttfb_ms`, under the
  provider's name and its model: once, at the first text or reasoning of a
  stream, never at a call's fragment, and as a whole Chat Completions
  answer arrives. The providers that left RoomKit's Rust core recorded it;
  these did not. In Python, the provider classes already recorded it.
- How an answer ended is told in the vocabulary of RoomKit's provider
  contract, `FinishReason`, in Rust and in Python: each provider maps its
  vendor's words (Anthropic's `end_turn` is `stop`, `tool_use` is
  `tool_calls`, `max_tokens` is `length`), and a word it does not know is
  no reason. `AiResponse::finish_reason` and `StreamDone::finish_reason`
  are `Option<FinishReason>`, where they were text.
- The providers are built on RoomKit's Rust core: what a provider is, the
  request it is asked (`AiContext`), the answer and the events it gives and
  its failure are `roomkit-ai`'s, at the revision the workspace names. This
  repository defines no contract of its own.
- `roomkit-providers-python-forms` (`bindings/python-forms`): the JSON
  forms of RoomKit's Python models, read into and written from the types of
  RoomKit's Rust core, for what crosses to Python and what a recorded case
  holds.
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
- `rust-examples`: fourteen RoomKit agents on these providers, each a
  program that checks what it shows against a local fake of its provider's
  wire (`make examples` runs them): a first agent on `OpenAi` and on
  `Anthropic`, a response schema, a fallback model, a streamed answer,
  tools, skills, MCP tools, and an agent served over WebSocket. They are the
  examples RoomKit's Rust core ran on its own providers, moved here with
  what they share, since the core holds no provider any more. Five of them
  ask a real model instead when `OPENAI_BASE_URL`, `OPENAI_API_KEY` and
  `OPENAI_MODEL` are set.
- `roomkit-providers-openai`: `DeepSeek`, `Kimi` and `MiniMax`, the
  providers of three vendors that speak Chat Completions with rules of their
  own, with `DeepSeekConfig`, `KimiConfig` and `MiniMaxConfig`: where a
  round's earlier reasoning goes back, how a reasoning setting is sent, the
  tool names the vendor's own endpoint takes, whether a response schema is
  taken, and DeepSeek's cache hits in the usage. They are the providers of
  RoomKit's Rust core for these vendors, on this crate's client; the wire
  cases do not cover them yet.
- `roomkit-providers-openai`: a conversation is rendered as Chat
  Completions messages by the renderer of RoomKit's Rust core
  (`roomkit_ai::chat_messages`), in place of the crate's own.
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
  completion fails, carrying what the body says of it, and no key, base URL
  or organization is read from the environment. A request takes again an
  idle connection for 5 seconds, one left idle is closed between 5 and 10
  seconds after its last request, and 100 are kept for a host at most. Every
  request carries `User-Agent: roomkit-providers/<version>`, which a
  `User-Agent` among `default_headers` replaces.
- `roomkit-providers-openai`: a provider goes through the proxies its
  environment names, `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` and
  `NO_PROXY`, read once, when it is built, as RoomKit's Python client reads
  them on a table of 136 environments. A proxy's user and password are read
  as that client reads them and given to the proxy apart from its address,
  so that no character of theirs changes the host a request is sent to, and
  they appear in no failure. A SOCKS proxy is refused, as is a proxy whose
  name holds another sign than a letter, a digit, a hyphen, a dot or an
  underscore, or one at a link-local IPv6 address with a zone; the system's
  proxy settings are not read.
- `roomkit-providers-openai`: a server's certificate is checked against the
  authorities `SSL_CERT_FILE` or `SSL_CERT_DIR` name, else against the
  system's, as RoomKit's Python client checks it; a system that keeps no
  authority is given the public roots compiled in, and one whose store is
  there and does not read builds no provider (`the system's certificate
  store at <path> cannot be read`). A certificate that is refused fails as
  `the provider's certificate is not trusted`.
- `roomkit-providers-openai`: the provider's streams,
  `generate_structured_stream` and `generate_stream`. A stream is pulled,
  and dropping it stops the request and closes its connection. It fails at
  a line or an event past 1 MiB and past 256 MiB in all, and one that stalls
  ends at the configuration's timeout. A stream that ends with neither a
  finish reason nor its end mark fails after what came, and hands no whole
  call; one whose body ends inside an event is read as one that ended
  before that event; a finish reason that is an empty text says nothing, and is no stop;
  a JSON error object in place of a stream fails as the status it names; a
  byte order mark that starts a stream is ignored.
- `roomkit-providers-openai`: `list_models`, the models the endpoint lists,
  each filled from the crate's catalogue where it knows the model.
- `roomkit-providers-openai`: a request the server refuses fails as
  `the provider answered HTTP <status> (<type>, <code>): <message>`, from
  the body's error object alone, or from what a server words its refusal
  with under `detail`: a text, or a list, of which each entry's `msg` and
  the place its `loc` names are said. A body that is no JSON is not echoed. The
  failure says the context overflowed when the body says so, by its code or
  by its words.
- `roomkit-providers-openai`: no failure's text repeats a secret of the
  configuration a server sends back (the key, a default header's value, a
  base URL's user information), nor the key a host lends one request, as it
  is, as a URL writes it or as the failure's own code writes it again, nor a
  control character or a character that shows nothing; a secret shorter than
  8 characters is not searched for. The README says what a failure repeats
  of an answer, and the bounds of a stream's calls.
- `roomkit-providers-openai`: a base URL with user information is sent with
  it as Basic credentials, in place of the key, as Python's client does. A
  base URL for which the client and Python's would not send the same
  request, and a timeout the clock cannot count, are refused when the
  provider is built.
- `roomkit-providers-openai`: `supports_vision` answers as Python's
  provider; the configuration's `vision` states it where the provider would
  not.
- `roomkit-providers-openai`: the Responses API, chosen with
  `OpenAiConfig::api` (`OpenAiApi::Responses`): stateless requests, an
  answer's reasoning items sent back inside one tool loop, and the tools
  under a namespace with `tool_namespace`.
- `roomkit-providers-openai`: `OpenAiConfig::token_source`, a host's source
  of renewable bearer tokens (`TokenSource`), asked before each try of a
  request, in place of the key.
- `roomkit-providers-openai`: two examples, which `make all` runs:
  `local_server` asks a stand-in server on a local port once whole and once
  streamed, and `behind_a_proxy` asks through a stand-in proxy named by the
  environment alone.
- `roomkit-providers-client`: the HTTP client a provider is built on, which
  `roomkit-providers-openai` is now written on: a request and its tries, the
  proxies and the certificate authorities read from the environment, the
  bounds of an answer and of a stream, the base URLs refused when a provider
  is built, the one exit of failures and, for a provider's tests, the replay
  of its recorded wire cases and a fake of an endpoint or of a proxy, over
  plain HTTP or over TLS. A provider crate holds its wire and nothing else.
  The failure of a generation leaves with its request, so that the key a
  host lends one request is in no failure of any provider, and the replay
  asks each generation again with a lent key that a refusal repeats. A
  provider gets the exit of its failures from its client (`Client::exit`),
  which adds the credentials of the proxies it read, and makes a header of
  its configuration with `configured_header`, whose value no `Debug` form
  shows; the `Debug` form of a `Call` shows the origin of its URL, the
  names of its headers and the size of its body. `Timeouts::new` is the
  only way to make timeouts. A provider a test points at a fake, and one a
  replay drives, go through no proxy of the environment. `catalogue` reads
  a model catalogue and the page of a listing, once for every provider.
- `roomkit-providers-anthropic`: `AnthropicConfig`, what a provider for
  Anthropic's Messages API is configured with: the names, the defaults and
  the JSON form of `AnthropicConfig` in RoomKit's Python implementation, the
  key written masked. Its `Debug` form prints neither the key, nor an extra
  header's value, nor what a base URL holds beside its scheme, host and
  port. The crate's README lists where it differs from Python's.
- `roomkit-providers-anthropic`: `Anthropic`, a provider for Anthropic's
  Messages API and the servers that speak it behind a base URL: whole
  answers (`generate`), streams (`generate_structured_stream`,
  `generate_stream`) and the model listing (`list_models`). It does as
  `AnthropicAIProvider` of RoomKit's Python implementation on 309 of the 377
  cases captured from it; its README says why it differs on the 68 others,
  reason by reason. It is written on `roomkit-providers-client`, as the OpenAI
  provider is, with the same guarantees: a redirect is refused, no secret
  appears in a failure, every size a server controls is bounded, a stream
  that ends with neither a stop reason nor the event that ends its message
  fails after what came, a JSON error object in place of a stream fails as
  the status its type names, and no key, base URL or header is read from
  the environment. The key of one request goes in that request's header
  alone. `make all` runs its example, `local_messages`, against a stand-in
  server on a local port.
- `roomkit-providers-anthropic`: a request the server refuses fails with the
  message of the API's error object as its whole text
  (`messages.2.content.1: Invalid signature in thinking block`), its status
  in `status_code`; a body that states no message is not echoed, and the
  text is then the status and its reason.
- A request that got no answer in time fails as `Request timed out.`, the
  words of RoomKit's Python implementation.
- `roomkit-providers`, the Python package (CPython 3.12 or newer, Linux
  x86_64, built from the repository), for RoomKit, which it depends on
  (0.117): `roomkit_providers.llm.openai` holds `OpenAI` and `OpenAIConfig`,
  the Rust OpenAI provider behind a native extension. `OpenAI` is a RoomKit
  `AIProvider`, to give an `AIChannel` in place of RoomKit's own: it takes
  an `AIContext`, gives RoomKit's models, and raises RoomKit's
  `ProviderError` and `ResponseSchemaError`. A context's metadata does
  not cross to the provider, and the credential of one request crosses
  apart, and is in no failure of that request. A context the contract
  refuses, or one with no JSON form, fails before it is sent, with RoomKit's
  `ProviderError` under the provider's label; so does a defect of the native
  code, wherever it shows, and a stream that closes without its end. A
  refused configuration leaves no secret in a traceback's frames, and a
  configuration's refusal never shows the value refused. The calls run on
  four threads of the package's own, whatever their number. A child forked
  after its parent's first call is refused in Python, before it enters the
  native code. Cancelling a call's task, or closing a stream, closes its
  connection. There is no `transport` parameter. The guide
  (`guide/python.md`) says how to switch and what differs.
- `roomkit-providers`: the time to the first token is recorded under
  RoomKit's name, `roomkit.llm.ttfb_ms`, by one rule for every provider: a
  stream at its first text or reasoning that is not empty, a whole answer
  when it came, if it holds text or reasoning. A whole answer of tool calls
  alone records none, where RoomKit's OpenAI provider records one.
- `roomkit-providers`: what a provider's module holds is the members of its
  configuration and one class statement. How a provider is built, what
  guards it, and every entry into the native module are written once for
  every provider.
- `roomkit-providers`: `roomkit_providers.llm.anthropic` holds `Anthropic`
  and `AnthropicConfig`, the Rust Anthropic provider as a RoomKit
  `AIProvider`, to give an `AIChannel` in place of RoomKit's own. It takes
  RoomKit's `AnthropicConfig` too, sends the key a context lends one
  request in that request alone, and replays through the package the cases
  recorded on RoomKit's Python provider. It was added with no change to what
  every provider shares. The guide says what differs for a host that
  switches.
- `roomkit-providers-http` depends on no HTTP client: it reads a host with
  the `url` crate, the parser `reqwest` reads a URL with.
- `roomkit-providers-http` is licensed `MIT AND Unicode-3.0`: its list of
  the characters that normalize to a delimiter of a URL is derived from
  Unicode data, and the crate ships that data's license, `LICENSE-UNICODE`,
  beside its own.
