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
- `roomkit-providers-llm`: for a provider crate, RoomKit's two rules for a
  text a model reads that neither the host nor the provider wrote: `fence`,
  which sets a text apart in a block it cannot close, and `identifier`,
  which writes a name outside any quote. Both decide as RoomKit's Python on
  a table of 275 texts and 35 names.
- `roomkit-providers-llm` is licensed `MIT AND Unicode-3.0`: its table of
  the characters Python's `repr` writes as they are, and the tables it reads
  a text from outside with, are derived from Unicode data, and the crate
  ships that data's license, `LICENSE-UNICODE`, beside its own.
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
  is or as the failure's own code writes it again, nor a control character
  or a character that shows nothing; a secret shorter than 8 characters is not searched for. The
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
