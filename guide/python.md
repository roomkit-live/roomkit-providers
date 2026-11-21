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

## One import per provider

The providers are filed by kind, then by vendor: `roomkit_providers.llm`
holds those of language models, one module each. A module holds the
provider's class, named for the vendor, and its configuration:

| Module | Class | Configuration |
| --- | --- | --- |
| `roomkit_providers.llm.openai` | `OpenAI` | `OpenAIConfig` |

`OpenAI` serves OpenAI's Chat Completions and the servers that speak it
behind a base URL.

## Switching from RoomKit's own provider

Change the import and the class's name, then check three things the
provider takes from nowhere but its configuration:

- **`OPENAI_BASE_URL`, `OPENAI_ORG_ID` and `OPENAI_PROJECT_ID` are not
  read.** RoomKit's provider reads them through the `openai` SDK. A host
  that relied on `OPENAI_BASE_URL` to reach another server, and gives no
  `base_url`, now reaches OpenAI's own endpoint and sends it its key: write
  the server in `base_url`.
- **No proxy is read from the environment.** RoomKit's provider goes through
  `HTTP_PROXY` and `HTTPS_PROXY`; this one connects straight to its
  endpoint, and behind a proxy it reaches nothing.
- **Only the public certificate roots compiled into the package are
  trusted.** An endpoint whose certificate is signed by a private authority
  cannot be reached, whatever the system's store or `SSL_CERT_FILE` hold.

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

What RoomKit records of a provider does not change with it. `name` is
`OpenAIAIProvider`, the name RoomKit's own provider goes by: the
`ai_provider` RoomKit stores with an answer, and the `provider` its
telemetry labels a turn with, stay what they were. The time to the first
token is recorded as RoomKit's provider records it: the metric
`roomkit.llm.ttfb_ms`, in `ms`, labelled with that name and the model,
through the telemetry the channel gives its provider. A stream records it
once, at its first text or reasoning, and never at a fragment of a call.

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
all but the key.

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
  configuration. What a provider it inherited says of itself still
  answers, and so does the catalogue (`available_models()`); `close()` does
  nothing, and the child's exit waits for nothing.
  Make the first call after forking, or use the `spawn` or `forkserver`
  start method.

## Errors

| Exception | When |
| --- | --- |
| RoomKit's `ProviderError` (`roomkit.providers.ai.ProviderError`) | A call failed, a context was refused before it was sent, or a provider could not be built. Its attributes: `retryable`, `provider`, `status_code`, `context_overflow`, `before_request`. Its text is `error.args[0]`; `str()` leads it with the provider and the status: `openai (401): ...` |
| RoomKit's `ResponseSchemaError` (`roomkit.providers.ai.ResponseSchemaError`) | A `ProviderError`: a response schema was set and no answer satisfying it can be returned. `reason` is `unsupported`, `refusal`, `truncated` or `invalid_json` |
| `ValueError` | A configuration's value the provider cannot work with. A configuration pydantic refuses is its `ValidationError`, a `ValueError` too |
| `TypeError` | A configuration that is not the provider's own: that of another provider, one derived from it |
| `RuntimeError` | A provider used in a forked child, or a call once the interpreter is exiting. `close()` raises neither |

## What a RoomKit user sees differently

What is sent and read is the Rust provider's. Its README lists
[where it differs on the wire](../crates/roomkit-providers-openai/README.md)
from RoomKit's Python provider; on a failure, for one, the text holds the
message, the type and the code of the server's error object, or what the
server says under `detail`, not its whole body. Beside the wire:

- **Another class name.** The class is `OpenAI`, where RoomKit's is
  `OpenAIAIProvider`: `type(provider).__name__`, which an AI channel's
  `info` shows, changes. The provider's `name` does not.
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
  short: a stream that closes without its end fails.
- **A response's `tasks` and `observations` stay empty**: the provider fills
  neither, as RoomKit's does not.
- **The time to the first token of `generate`** is recorded when the call
  gave an answer. RoomKit's records it as soon as the server answered, also
  when that answer is then refused: an error object answered with a 200, a
  response schema the answer does not satisfy.
- **`list_models` fails with RoomKit's `ProviderError`**, where RoomKit's
  provider raises the openai SDK's error or a raw exception.
- **`close()`** releases nothing by itself: the connections close when the
  provider is no longer held. RoomKit's closes its HTTP client.
- **A forked process and the interpreter's exit** are refused with a
  `RuntimeError`, as [above](#cancellation-and-lifetime).
- **Its own threads.** The provider runs its calls on four threads, looks
  names up on four more at most, and hands what the calls give to Python on
  one, whatever the number of calls. It keeps an idle connection 5 seconds,
  and 100 of them for a host at most, as RoomKit's client does.
- **The `openai` SDK is not needed.** The provider runs without RoomKit's
  `openai` extra.

## What is private

A name that starts with an underscore is private, the native module
`roomkit_providers._native` first, and so is a module whose name does: it
may change with no notice. The public names are those a provider's module
exports.

The native module carries one function for this repository's tests alone:
it builds a provider that sends to a server on the local machine and keeps
its waits instead of taking them, which the tests replay the recorded wire
cases through. It is no way to configure a provider.
