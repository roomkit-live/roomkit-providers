# Python

`roomkit-providers` is the Python package of this repository: the Rust
providers behind a native extension, for asynchronous programs. It needs
CPython 3.12 or newer and has no Python dependency. RoomKit is not required:
the adapter that plugs a provider into a RoomKit agent is not written yet.

**Early development.** Nothing is published. The package is built from this
repository, for Linux x86_64.

## Installation

Build a wheel, then install it in your environment:

```sh
make python-wheel          # writes dist/roomkit_providers-*.whl
pip install dist/roomkit_providers-*.whl
```

The build needs the Rust toolchain of `rust-toolchain.toml` and
[uv](https://docs.astral.sh/uv/), which runs maturin. The wheel is built for
the stable ABI: one wheel serves CPython 3.12 and every later version.

## A first call

```python
import asyncio

from roomkit_providers import AIRequest
from roomkit_providers.openai import OpenAIAIProvider, OpenAIConfig


async def main() -> None:
    config = OpenAIConfig(
        api_key="placeholder-not-a-key",
        model="local-model",
        base_url="http://localhost:11434/v1",
    )
    provider = OpenAIAIProvider(config)
    request: AIRequest = {"messages": [{"role": "user", "content": "Hello"}]}

    response = await provider.generate(request)
    print(response["content"], response["usage"])

    async for event in provider.generate_structured_stream(request):
        if event["type"] == "text_delta":
            print(event["text"], end="")


asyncio.run(main())
```

`examples/local_server.py` does the same against a stand-in server it starts
on a local port, with no key and no network.

## Requests and answers are plain dicts

A request, a response, a stream event and a model's entry are plain dicts in
the contract's JSON form: the one RoomKit's models give with
`model_dump(mode="json")`. The package defines no class for them. It names
their members for a type checker, with typed dicts called as RoomKit calls
its models:

| Type | What it is |
| --- | --- |
| `AIRequest` | What one generation is asked: `messages`, `system_prompt`, `temperature`, `max_tokens`, `thinking_budget`, `enable_thinking`, `reasoning_effort`, `tools`, `response_schema`, `metadata`. Every member may be left out |
| `AIMessage` | A message: `role`, `content` (a text, or a list of parts), `metadata` |
| `AITextPart`, `AIImagePart`, `AIToolCallPart`, `AIToolResultPart`, `AIThinkingPart` | The parts of a message, each with its `type` |
| `AITool` | A tool the model may call |
| `AIResponse`, `AIToolCall`, `ServedCall` | A whole answer and its calls |
| `StreamTextDelta`, `StreamThinkingDelta`, `StreamToolCallDelta`, `StreamToolCall`, `StreamUsage`, `StreamDone` | The events of a stream, each with its `type`; `StreamEvent` is any of them |
| `ModelInfo`, `ModelPricing` | What is known of a model |

Nothing checks these types when the program runs. A request is read by the
contract's own reader when the call is made: a member left out takes its
default (`temperature` is 0.7), and what does not fit is a `ValueError`,
before anything is sent. The reader is stricter than RoomKit's models: it
converts nothing (`"0.7"` is no number, `1` is no boolean), and refuses a
role that is not `system`, `user`, `assistant` or `tool`.

What a provider gives back holds every member, `None` included.

## The configuration

`OpenAIConfig` has the fields and the defaults of RoomKit's `OpenAIConfig`:

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
    use_max_completion_tokens=None,            # None: decided from the model
    supports_custom_temperature=None,          # None: decided from the model
    reasoning_effort=None,
    default_headers=None,
    extra_body=None,
    supports_response_schema=None,
    supports_response_schema_with_tools=None,
    supports_reasoning_effort_with_tools=None,
    thinking_field=None,
)
```

It is checked when it is built, and a value that does not fit is a
`ValueError`. It cannot be changed afterwards. `repr` never shows the key,
the value of a default header, or what a base URL holds before its last `@`.

## The provider

`OpenAIAIProvider(config)` has the members of RoomKit's `AIProvider`:

| Member | |
| --- | --- |
| `name`, `model_name` | Properties: the class name, and the configured model |
| `supports_vision`, `supports_streaming`, `supports_structured_streaming`, `supports_response_schema`, `supports_response_schema_with_tools`, `supports_deferred_tools` | Properties |
| `context_window`, `catalog_entry()` | The configured model's window and its entry in the catalogue, `None` for a model the catalogue does not know |
| `available_models()` | A class method: the curated catalogue, with no request |
| `await list_models()` | The models the endpoint lists |
| `await generate(request)` | One whole answer |
| `generate_stream(request)` | An async iterator over the answer's text |
| `generate_structured_stream(request)` | An async iterator over the answer's events |
| `await close()` | Awaited when the provider is no longer needed |

The provider's own README,
[`crates/roomkit-providers-openai`](../crates/roomkit-providers-openai/README.md),
says what it guarantees: the secrets a failure never repeats, the bounds on
what a server sends, where a request goes, and the timeouts.

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
- **Event loops and threads.** One provider serves several event loops, one
  after the other or on several threads at once. Each call completes on the
  loop that made it. An answer that comes after its loop was closed is
  dropped.
- **The interpreter's exit** stops the calls still under way, which closes
  their connections, and waits for them to end. No call is made afterwards.
- **A forked process.** A child forked after its parent's first call cannot
  make one: the threads that carry the calls are not copied. It is refused
  at once with a `RuntimeError`. Make the first call after forking, or use
  the `spawn` or `forkserver` start method.

## Errors

| Exception | When |
| --- | --- |
| `ProviderError` | A call failed. Its attributes are RoomKit's: `retryable`, `provider`, `status_code`, `context_overflow`, `before_request`; `message` is the text. `str()` leads it with the provider and the status: `openai (401): ...` |
| `ResponseSchemaError` | A `ProviderError`: a response schema was set and no answer satisfying it can be returned. `reason` is `unsupported`, `refusal`, `truncated` or `invalid_json` |
| `ValueError` | A configuration or a request the contract refuses as a value |
| `TypeError` | A request that cannot be written as JSON, a configuration built with an unknown or a missing argument |
| `RuntimeError` | A call in a forked child, or once the interpreter is exiting |

These are the package's own classes: `except roomkit.ProviderError` does not
catch them.

## The key of one request

RoomKit lets a host leave the credential of one request under `api_key` in a
request's `metadata`. The OpenAI provider does not read it, here as in
RoomKit: the configuration's key is the one sent. A request crosses to the
native module as it is given, that entry included, and nothing of
`metadata` is sent to the server.

## Where the package differs from RoomKit's provider

What the provider sends and reads is the Rust provider's, which replays the
cases captured on RoomKit's Python provider; its README lists where it
differs on the wire. The Python surface differs in this:

- **Plain dicts, not models.** `generate` takes an `AIRequest` dict where
  RoomKit takes an `AIContext`, and its parameter is named `request`. An
  answer, an event and a model's entry are dicts. A response has no `tasks`
  and no `observations`, which RoomKit adds.
- **No `transport` parameter.** `OpenAIAIProvider(config)` takes the
  configuration alone.
- **Not RoomKit's base class.** The provider does not inherit `AIProvider`,
  has none of its private hooks, and records no telemetry. It is not made to
  be subclassed: what it sends is decided in Rust.
- **Its own exceptions**, with a `message` attribute RoomKit's does not have.
- **`list_models` fails with a `ProviderError`**, where RoomKit raises the
  openai SDK's error or a raw exception.
- **`close()`** releases nothing by itself: the connections close when the
  provider is no longer held. RoomKit's closes its HTTP client.
- **The configuration is no pydantic model.** It is a frozen dataclass: no
  `model_dump`, no `model_copy` (`dataclasses.replace` copies one). `api_key`
  is a `str`, where RoomKit's is a `SecretStr`. An unknown argument is a
  `TypeError`, where pydantic ignores it. Nothing is converted: `"30"` for
  a number, `1024.0` for an integer and `1` for a boolean are refused.
- **The two switches** `use_max_completion_tokens` and
  `supports_custom_temperature` default to `None`, a switch left unset,
  where RoomKit's signature says `False` and `True`. Both are read back
  decided, with the same values as RoomKit's.
- **A request is checked when the call is made**, not when it is built.
