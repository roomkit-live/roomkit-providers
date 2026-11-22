# Python

`roomkit-providers` is the Python package of this repository: RoomKit's
model providers, implemented in Rust behind a native extension. A provider
is a RoomKit `AIProvider`: an AI channel takes it as it takes RoomKit's own.

It needs CPython 3.12 or newer, and RoomKit, which it depends on.

**Early development.** Nothing is published. The package is built from this
repository, for Linux x86_64.

## Installation

The package asks for `roomkit>=0.117.0.dev0,<0.118`, which no index holds
yet: install RoomKit from its repository first, then build a wheel and
install it:

```sh
pip install "roomkit @ git+https://github.com/roomkit-live/roomkit"
make python-wheel          # writes dist/roomkit_providers-*.whl
pip install dist/roomkit_providers-*.whl
```

The build needs the Rust toolchain of `rust-toolchain.toml` and
[uv](https://docs.astral.sh/uv/), which runs maturin. The wheel is built for
the stable ABI: one wheel serves CPython 3.12 and every later version.

The package is tested against RoomKit 0.117.0.dev0, the development version
of RoomKit's repository, and against no other.

## In a RoomKit agent

```python
from roomkit import AIChannel, RoomKit
from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

config = OpenAIConfig(
    api_key="placeholder-not-a-key",
    model="local-model",
    base_url="http://localhost:11434/v1",
)
kit = RoomKit()
kit.register_channel(AIChannel("assistant", provider=OpenAI(config)))
```

`examples/roomkit_agent.py` is a whole agent: it answers a message, calls a
tool on the way and streams its answer, against a stand-in server it starts
on a local port. `examples/local_server.py` asks the provider alone, as
RoomKit asks one: a context in, RoomKit's models out.
`examples/anthropic_agent.py` is the same agent over `Anthropic`: the model
reasons, calls a tool, and the round goes back with its signed reasoning.

## One import per provider

The providers are filed by kind, then by vendor: `roomkit_providers.llm`
holds those of language models, one module each. A module holds the
provider's class, named for the vendor, and its configuration:

| Module | Class | Configuration |
| --- | --- | --- |
| `roomkit_providers.llm.openai` | `OpenAI` | `OpenAIConfig` |
| `roomkit_providers.llm.anthropic` | `Anthropic` | `AnthropicConfig` |

`OpenAI` serves OpenAI's Chat Completions and the servers that speak it
behind a base URL; `Anthropic`, Anthropic's Messages API and the servers
that speak it. What this guide says of `OpenAI` holds for `Anthropic`, with
its own names; [Anthropic](#anthropic) says what is its own.

## Switching from RoomKit's own provider

Change the import and the class's name. The network of the host stays as
it is: the provider reads from the environment what RoomKit's provider
reads there of proxies and of certificates, once, when it is built.

- **Proxies.** `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY` are
  read as RoomKit's provider reads them, in upper or in lower case: a
  request goes through the proxy named for its endpoint's scheme, unless
  `NO_PROXY` exempts the host. A proxy's URL may carry a user and a
  password, which are sent to the proxy alone and appear in no failure.
- **Certificates.** A server's certificate is checked against the
  authorities of the file `SSL_CERT_FILE` names, else against those filed in
  the directories `SSL_CERT_DIR` names, else against the system's own. An
  endpoint whose certificate a private authority signed is reached as it is
  with RoomKit's provider.
- **When.** The environment is read when `OpenAI(...)` is called. A variable
  set or changed afterwards, in `os.environ` as anywhere else, changes
  nothing for that provider; one built later reads it.

Then check what does differ:

- **`OPENAI_BASE_URL`, `OPENAI_ORG_ID` and `OPENAI_PROJECT_ID` are not
  read.** RoomKit's provider reads them through the `openai` SDK. A host
  that relied on `OPENAI_BASE_URL` to reach another server, and gives no
  `base_url`, now reaches OpenAI's own endpoint and sends it its key: write
  the server in `base_url`.
- **A SOCKS proxy is refused.** RoomKit's provider takes `socks5://` once
  the `socksio` package is installed; this one is not built in an
  environment that names one, and says so.
- **The system's proxy settings are not read.** On macOS and on Windows,
  RoomKit's provider takes the proxies of the system's settings when no
  proxy variable is set; this one reads the variables alone.
- **A system with no certificate store** (a container without one) gets the
  public roots compiled into the package, where RoomKit's provider trusts
  nothing there. They are never added to a store that holds an authority.
- **A system certificate store that is there and cannot be read**: the
  provider is not built, and says which path, where RoomKit's provider is
  built all the same.
- **A proxy whose name holds another sign** than a letter, a digit, a
  hyphen, a dot or an underscore, or one at a link-local IPv6 address with a
  zone, is refused when the provider is built.
- **A certificate that is refused** fails in one of three sentences, where
  RoomKit's provider says `Connection error.` for each: `the provider's
  certificate is not trusted`, `the provider's certificate is not for its
  host name`, or `the provider's certificate has expired, or is not valid
  yet`. Both providers say the request is worth trying again.
- **A server's own certificate named by `SSL_CERT_FILE`** is not trusted: a
  certificate a server signed itself is no authority here, where RoomKit's
  provider trusts the server that shows it. Give the server a certificate
  issued by an authority of your own, and name that authority's
  certificate in the variable.

The package was run on Linux alone: what it does on macOS and on Windows,
where the system itself checks a certificate, was not verified.

```python
# from roomkit.providers.openai import OpenAIAIProvider, OpenAIConfig
from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

provider = OpenAI(OpenAIConfig(api_key=..., model=...))
```

`OpenAI` also takes RoomKit's own `OpenAIConfig`, so that a host moves its
provider first and its configuration when it wants. The members are
RoomKit's, with RoomKit's types: `generate(context)` takes an `AIContext`
and gives an `AIResponse`, `generate_structured_stream(context)` yields
RoomKit's stream events, `generate_stream(context)` the answer's text,
`list_models()` and `available_models()` give `ModelInfo`, and
`catalog_entry()`, `context_window`, `model_name` and the `supports_*`
properties answer as RoomKit's provider does for the same configuration. A
failure is RoomKit's `ProviderError` or `ResponseSchemaError`, with the same
attributes.

What RoomKit records under a provider's `name` does not change with it.
`name` is `OpenAIAIProvider`, the name RoomKit's own provider goes by: the
`ai_provider` RoomKit stores with an answer, and the `provider` the metric
of the first token is labelled with, stay what they were. One label does
change: RoomKit labels the span of a generation, `llm.generate`, and an AI
channel's `info`, with the name of the provider's class, which is `OpenAI`
here.

### The time to the first token

Every provider of the package records it by one rule, as the metric
`roomkit.llm.ttfb_ms`, in `ms`, labelled with the provider's `name` and the
model, through the telemetry the channel gives its provider:

- **a stream** records it once, at its first text or reasoning that is not
  empty: never at a signature alone, a redacted block of reasoning, or a
  fragment of a call;
- **a whole answer** (`generate`) records it when the answer came, if the
  answer holds text or reasoning. An answer made of calls alone records
  nothing, and neither does a call that failed.

Two of RoomKit's own providers record a whole answer otherwise:

- RoomKit's OpenAI provider records it for every whole answer, one of tool
  calls alone too: with this package such an answer no longer records
  `roomkit.llm.ttfb_ms`. RoomKit's also records it as soon as the server
  answered, when that answer is then refused: an error object answered with
  a 200, a response schema the answer does not satisfy.
- RoomKit's Anthropic provider reads a stream inside `generate()` and
  records its first text or reasoning. Here the value of a whole answer is
  the time the whole answer took. RoomKit's has so recorded it for a
  `generate()` that then fails, after its first text: with this package
  such a call records nothing.

## The configuration

`OpenAIConfig` is a pydantic model with the fields, the defaults and the
validation of RoomKit's `OpenAIConfig`:

```python
OpenAIConfig(
    *,
    api_key,                                   # required; "" sends no Authorization
    base_url=None,                             # None is OpenAI's own endpoint
    model,                                     # required
    max_tokens=1024,
    timeout=30.0,
    connect_timeout=5.0,
    max_retries=0,
    include_stream_usage=True,
    use_max_completion_tokens=False,           # unset: decided from the model
    supports_custom_temperature=True,          # unset: decided from the model
    reasoning_effort=None,
    default_headers=None,
    extra_body=None,
    supports_response_schema=None,
    supports_response_schema_with_tools=None,
    supports_reasoning_effort_with_tools=None,
    thinking_field=None,
)
```

It reads what RoomKit's reads, converts what RoomKit's converts, ignores an
argument it does not know and refuses what RoomKit's refuses, with
pydantic's `ValidationError`. The two switches a modern model decides on
OpenAI's own endpoint are decided when the configuration is built, unless
they were stated.

`repr` never shows the key, the value of a default header or of a member of
`extra_body`, nor more of a base URL than its scheme, its host and its
port: a path and a query may hold a gateway's token. RoomKit's own shows
all but the key. A refusal says what is refused and never shows the value
given, where pydantic's shows it by default, and RoomKit's with it: a
`ValidationError` here ends each line with `[type=...]` alone.

A base URL Python's URL reader refuses (a bracket that holds no address, a
character that reads as a delimiter of a URL) is refused when the
configuration is built, as RoomKit's refuses it, in words that do not repeat
the URL.

What a configuration does not hide:

- **A configuration its class refuses leaves what it was given in the
  frames of that refusal**: in pydantic's own, as RoomKit's does, and in
  the package's own `__init__` too. A traceback written with its frames'
  locals (`capture_locals`) shows it all there: a key given as text, the
  value of a header, a base URL with its password, a member of
  `extra_body`. A key given as a `SecretStr` is not shown; nothing covers
  the others. Where a configuration is built from such values, do not
  write a traceback's locals. A configuration the provider refuses as it is
  built, and a call that fails, leave none of them in a frame.
- **`model_dump()` and `model_dump_json()` show** the value of a header, a
  member of `extra_body` and the base URL whole, its password with it, as
  RoomKit's configuration does. They mask the key alone. `repr` is the form
  that hides the others.
- **A member of `extra_body` is no secret to a failure.** A server that
  repeats one in the message of its refusal is repeated: a failure's text is
  cleaned of the key, of the key a context lends, of a default header's
  value and of a base URL's user information.

The provider reads its configuration once, when it is built. A value the
provider cannot work with is refused then: a time below zero is a
`ValueError`, a base URL the provider would not send to is a
`ProviderError`.

## The provider

`OpenAI(config)` has the members of RoomKit's `AIProvider`:

| Member | |
| --- | --- |
| `name`, `model_name` | Properties: the name RoomKit records, and the configured model |
| `supports_vision`, `supports_streaming`, `supports_structured_streaming`, `supports_response_schema`, `supports_response_schema_with_tools`, `supports_deferred_tools` | Properties |
| `context_window`, `catalog_entry()` | The configured model's window and its entry in the catalogue, `None` for a model the catalogue does not know |
| `available_models()` | A class method: the curated catalogue, with no request |
| `await list_models()` | The models the endpoint lists |
| `await generate(context)` | One whole answer |
| `generate_stream(context)` | An async iterator over the answer's text |
| `generate_structured_stream(context)` | An async iterator over the answer's events |
| `await close()` | Awaited when the provider is no longer needed |

The provider's own README,
[`crates/roomkit-providers-openai`](../crates/roomkit-providers-openai/README.md),
says what it guarantees: the secrets a failure never repeats, the bounds on
what a server sends, where a request goes, and the timeouts.

### What crosses of a context

A context is written once, as JSON, for the Rust provider: its `messages`,
`system_prompt`, `temperature`, `max_tokens`, `thinking_budget`,
`enable_thinking`, `reasoning_effort`, `tools` and `response_schema`, with
everything they carry: images, deferred tools, a tool result's references,
reasoning with its signatures.

Its `metadata` does not cross. No provider reads anything of it but the
credential a host lends one request (`api_key`), and a host or a hook may
leave there what has no JSON form. That credential crosses apart, never
inside the JSON, and reaches a provider that reads one. OpenAI's does not,
here as in RoomKit: the configuration's key is the one sent.

### Streams

Nothing is sent before a stream's first item is asked for. From then on the
answer is read as it comes, at most 64 items ahead of those the program
took: a program that stops taking them stops the reading there. A failure is
raised after the items read before it.

Each time the stream is asked, it hands over every item already received,
and never waits to gather more: a burst of events costs a few crossings
between Rust and Python, not one each.

## Cancellation and lifetime

- **Cancelling the task** that awaits `generate`, `list_models` or the next
  item of a stream stops the request and closes its connection.
- **Closing a stream** (`await stream.aclose()`), leaving its `async for`
  with `break`, or no longer holding it closes its connection too.
- **The event loop is never held.** Every call that waits returns an
  awaitable; other coroutines run meanwhile.
- **A loop that is closed without cancelling its calls** does not stop
  them: each runs to its end or to its timeout, and what it gives is
  dropped. Cancel a call to stop it.
- **Event loops and threads.** One provider serves several event loops, one
  after the other or on several threads at once. Each call completes on the
  loop that made it. An answer that comes after its loop was closed is
  dropped.
- **The interpreter's exit** stops the calls still under way, which closes
  their connections, and waits for them to end. No call is made afterwards.
- **A forked process.** A child forked after its parent's first call cannot
  use a provider: the threads that carry the calls are not copied. The
  package refuses it in Python, with a `RuntimeError`, before it enters the
  native code: a call, a stream, a listing, a new provider or a new
  configuration, and the first import of a provider's module, whose class
  statement reads the catalogue. What a provider it inherited says of itself still
  answers, and so does the catalogue (`available_models()`); `close()` does
  nothing, and the child's exit waits for nothing.
  Make the first call after forking, or use the `spawn` or `forkserver`
  start method.

## Errors

| Exception | When |
| --- | --- |
| RoomKit's `ProviderError` (`roomkit.providers.ai.ProviderError`) | A call failed, a context was refused before it was sent, a provider could not be built, or the native code stopped on a defect, wherever it did: in a call, while a provider or a configuration is built. Its attributes: `retryable`, `provider`, `status_code`, `context_overflow`, `before_request`. Its text is `error.args[0]`; `str()` leads it with the provider and the status: `openai (401): ...` |
| RoomKit's `ResponseSchemaError` (`roomkit.providers.ai.ResponseSchemaError`) | A `ProviderError`: a response schema was set and no answer satisfying it can be returned. `reason` is `unsupported`, `refusal`, `truncated` or `invalid_json` |
| `ValueError` | A configuration's value the provider cannot work with. A configuration pydantic refuses is its `ValidationError`, a `ValueError` too |
| `TypeError` | A configuration that is not the provider's own: that of another provider, one derived from it |
| `RuntimeError` | A provider or a configuration used in a forked child, a call once the interpreter is exiting, or a call once the package lost the thread that hands its calls to Python (a defect, after which no call can complete in that process). `close()` raises neither |

## What a RoomKit user sees differently

What is sent and read is the Rust provider's. Its README lists
[where it differs on the wire](../crates/roomkit-providers-openai/README.md)
from RoomKit's Python provider; on a failure, for one, the text holds the
message, the type and the code of the server's error object, or what the
server says under `detail`, not its whole body. Beside the wire:

- **Another class name.** The class is `OpenAI`, where RoomKit's is
  `OpenAIAIProvider`: `type(provider).__name__` changes, which an AI
  channel's `info` shows and RoomKit labels the `llm.generate` span with.
  The provider's `name` does not.
- **A refused configuration says what is refused and never shows the value
  given**, where RoomKit's `ValidationError` shows it, a key given as text
  included: a line ends with `[type=missing]` here, with
  `[type=missing, input_value={'api_key': '...'}, input_type=dict]` there.
- **No `transport` parameter.** `OpenAI(config)` takes the configuration
  alone. The provider builds its own HTTP client: a host that gave RoomKit's
  provider a transport, to pin addresses or to answer a test, has no place
  to give it here.
- **OpenAI's own configuration only.** The configuration of a provider
  derived from OpenAI's (a subclass of either `OpenAIConfig`) is refused
  with a `TypeError`: that provider's rules are not this one's.
- **Not made to be subclassed.** The provider has none of the private hooks
  RoomKit's is derived through, and no `openai` client: what it sends is
  decided in Rust. `provider._client` does not exist.
- **A context the contract refuses fails before it is sent**, with RoomKit's
  `ProviderError`, `before_request` set, which an AI channel catches. Each
  is one RoomKit's provider sends as it can: a message whose role is not
  `system`, `user`, `assistant` or `tool`; a `temperature` that is no
  number; a `max_tokens` below zero or from 2**32 on; a text that holds a
  lone surrogate; an object with no JSON form in a message's metadata or
  in a call's arguments.
- **Numbers past what JSON carries.** An integer outside 64 bits, in a
  tool's schema or in the arguments of a call a message replays, is read as
  a float and sent as one, where RoomKit keeps its digits: with a schema
  typed `integer`, the channel then refuses the call the model writes. A
  NaN among a call's arguments is sent as `null`.
- **A defect of the provider is a failure of the call.** A panic in the
  native code is RoomKit's `ProviderError`, final, never an answer cut
  short, and never an exception outside `Exception`: in a call, in a
  stream's reading, while a provider or a configuration is built. A stream
  that closes without its end fails.
- **The key a context lends one request** (`metadata["api_key"]`) is in no
  failure of that request, in any form its text may hold it in, as the
  configuration's is in none. A key shorter than 8 characters is not
  searched for.
- **A response's `tasks` and `observations` stay empty**: the provider fills
  neither, as RoomKit's does not.
- **The time to the first token** is recorded by one rule for every
  provider, which differs from RoomKit's own for a whole answer:
  [above](#the-time-to-the-first-token).
- **`list_models` fails with RoomKit's `ProviderError`**, where RoomKit's
  provider raises the openai SDK's error or a raw exception.
- **`close()`** releases nothing by itself: the connections close when the
  provider is no longer held. RoomKit's closes its HTTP client.
- **A forked process and the interpreter's exit** are refused with a
  `RuntimeError`, as [above](#cancellation-and-lifetime).
- **Its own threads.** The provider runs its calls on four threads, looks
  names up on four more at most, and hands what the calls give to Python on
  one, whatever the number of calls. That one thread hands every call that
  ended while it waited for the interpreter's lock under one hold of it;
  where other Python threads keep the lock busy, the calls wait for it as
  any Python thread does. A request takes again an idle connection for
  5 seconds, as with RoomKit's client, and 100 are kept for a host at most;
  one left idle is closed between 5 and 10 seconds after its last request.
- **`User-Agent`** is `roomkit-providers/<version>`, where RoomKit's
  provider sends the openai SDK's; a `default_headers` entry replaces it.
- **The `openai` SDK is not needed.** The provider runs without RoomKit's
  `openai` extra.

## Anthropic

```python
# from roomkit.providers.anthropic import AnthropicAIProvider, AnthropicConfig
from roomkit_providers.llm.anthropic import Anthropic, AnthropicConfig

provider = Anthropic(AnthropicConfig(api_key=..., model=...))
```

`AnthropicConfig` has the fields, the defaults and the validation of
RoomKit's `AnthropicConfig`, and `Anthropic` takes RoomKit's own too:

```python
AnthropicConfig(
    *,
    api_key,                             # required
    model,                               # required
    max_tokens=1024,
    timeout=60.0,
    connect_timeout=5.0,
    max_retries=0,
    base_url=None,                       # None is Anthropic's own endpoint
    extra_headers=None,
    enable_prompt_caching=True,
    use_adaptive_thinking=False,         # unset: decided from the model
    supports_custom_temperature=True,    # unset: decided from the model
)
```

A base URL is kept as RoomKit's configuration keeps it: without the white
space around it, the slashes that end it and a `/v1/messages` it ends with.
`repr` shows neither the key, nor an extra header's value, nor more of the
base URL than its scheme, its host and its port.

What this guide says of `OpenAI` holds for `Anthropic`: the members of an
`AIProvider`, what crosses of a context, streams, cancellation and lifetime,
the errors, the environment read when the provider is built. `name` is
`AnthropicAIProvider`, the name RoomKit's own provider goes by; the class
is `Anthropic`. What is Anthropic's own:

- **The key a context lends one request** (`metadata["api_key"]`) is sent,
  in `x-api-key`, in place of the configuration's, for that request alone.
  It is in no failure of that request. One HTTP client serves every key,
  where RoomKit's provider keeps a client of its SDK for each of the last
  eight keys.
- **A request with no key at all**, none in the configuration, none lent
  and no credential among the extra headers, fails before it is sent.
- **Reasoning** comes back as RoomKit's thinking parts, each with its
  signature, a redacted block as its opaque data, and goes back to the
  model unchanged in the next round.
- **Deferred tools.** `supports_deferred_tools` is the catalogue's answer
  for the model on Anthropic's own endpoint, and false behind another base
  URL.
- **The time to the first token of a whole answer** is the time the whole
  answer took, and a call that failed records none:
  [above](#the-time-to-the-first-token).
- **An empty or blank base URL** is refused when the provider is built,
  with a `ProviderError`. RoomKit's reads it as Anthropic's own endpoint:
  leave `base_url` out for that one.
- **On the wire** the provider does as RoomKit's on 309 of the 377 cases
  recorded from it, and says why it differs on each of the others: its
  README,
  [`crates/roomkit-providers-anthropic`](../crates/roomkit-providers-anthropic/README.md),
  lists them. For a host the ones that show most: a redirect is not
  followed; a stream that ends with neither a stop reason nor the event
  that ends its message fails, a failure worth a new request, where
  RoomKit's reads it as complete; `list_models` fails with RoomKit's
  `ProviderError`, where RoomKit's raises its SDK's error; a failure's text
  is the provider's own sentence, with the type and the message of the
  server's error object.
- **The `anthropic` SDK is not needed.** The provider runs without
  RoomKit's `anthropic` extra.

## What is private

A name that starts with an underscore is private, the native module
`roomkit_providers._native` first, and so is a module whose name does: it
may change with no notice. The public names are those a provider's module
exports.

The native module carries one function for this repository's tests alone:
it builds a provider that sends to a server on the local machine and keeps
its waits instead of taking them, which the tests replay the recorded wire
cases through. It is no way to configure a provider.
