# RoomKit Providers

**Model providers for AI applications, written in Rust.**

[![CI](https://github.com/roomkit-live/roomkit-providers/actions/workflows/ci.yml/badge.svg)](https://github.com/roomkit-live/roomkit-providers/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/roomkit-live/roomkit-providers/blob/main/LICENSE)
[![MSRV: 1.94](https://img.shields.io/badge/MSRV-1.94-orange.svg)](https://github.com/roomkit-live/roomkit-providers/blob/main/Cargo.toml)

RoomKit Providers is where the clients of model services live for the
[RoomKit](https://github.com/roomkit-live/roomkit) ecosystem: one
implementation per service, written once in Rust, for RoomKit's Rust core
and, behind a Python binding, for RoomKit's Python.

> **Early development.** The HTTP client the providers are built on is
> written, with providers for OpenAI, Anthropic, Gemini, Mistral, Ollama,
> PolarGrid and managed local llama.cpp: whole answers, streams and the model listing. A Python package exposes them
> behind a native extension, each as a RoomKit provider. Nothing is
> released. The
> [changelog](CHANGELOG.md) says what exists; the rest of this page says
> where the project is going.

For local GGUF inference, the [managed llama.cpp provider](crates/roomkit-providers-llamacpp/README.md)
installs the pinned server or runs an explicit binary. In Python, import
`LlamaCpp` and `LlamaCppConfig` from `roomkit_providers.llm.llamacpp`.

DeepSeek, Kimi, MiniMax, xAI, Cerebras, Qwen, Meta, OpenRouter, LiteLLM, Azure
and vLLM are in Python too, each in its module of `roomkit_providers.llm`:
`DeepSeek` and `DeepSeekConfig` from `.deepseek`, `Kimi` and `KimiConfig`
from `.kimi`, `MiniMax` and `MiniMaxConfig` from `.minimax`, `XAI` and
`XAIConfig` from `.xai`, `Cerebras` and `CerebrasConfig` from `.cerebras`,
`Qwen` and `QwenConfig` from `.qwen`, `Meta` and `MetaConfig` from `.meta`,
`OpenRouter` and `OpenRouterConfig` from `.openrouter`, `LiteLLM` and
`LiteLLMConfig` from `.litellm`, `AzureAI` and `AzureAIConfig` from
`.azure`, `VLLM` and `VLLMConfig` from `.vllm` (in place of RoomKit's
`create_vllm_provider`). Each takes RoomKit's own configuration of the
vendor as well.

Gemini, on Google's Developer API, is in Python too: `Gemini` and
`GeminiConfig` from `roomkit_providers.llm.gemini`, in place of RoomKit's
`GeminiAIProvider`. It takes RoomKit's own `GeminiConfig` as well.

Mistral, on its chat completions, is in Python too: `Mistral` and
`MistralConfig` from `roomkit_providers.llm.mistral`, in place of RoomKit's
`MistralAIProvider`. It takes RoomKit's own `MistralConfig` as well.

Ollama, on its native API, is in Python too: `Ollama` and `OllamaConfig`
from `roomkit_providers.llm.ollama`, in place of RoomKit's
`OllamaAIProvider`. It takes RoomKit's own `OllamaConfig` as well.

PolarGrid, on the chat completions of its edges, is in Python too:
`PolarGrid` and `PolarGridConfig` from `roomkit_providers.llm.polargrid`, in
place of RoomKit's `PolarGridAIProvider`. It takes RoomKit's own
`PolarGridConfig` as well. RoomKit's `available_regions` and
`connected_region` are on the Rust provider only, for now.

## Goals

- **One implementation per provider.** A provider's wire format, streaming,
  retries and failures are written once, in Rust.
- **What a provider is stays in the core.** The request, its messages and
  tools, the response, the events of a streamed answer, typed failures and
  model catalogues are RoomKit's: a provider here implements RoomKit's
  provider trait in Rust and is a RoomKit `AIProvider` in Python. This
  repository defines none of it.
- **The core is tied to no vendor.** A vendor's code lives here, so that
  RoomKit's cores name none.
- **The host stays in control.** Endpoints, credentials, timeouts and limits
  are explicit host configuration. A provider writes no credential into a
  log, a `Debug` form or a result, and removes from an error's text a
  credential of 8 characters or more that a server sends back, as it is, as
  a URL writes it or as the error's own code writes it again; its README
  states the limits.

Section numbers in the code and its documentation (`RFC §6.7`) refer to the
[RoomKit RFC](https://github.com/roomkit-live/roomkit-specs).

## Planned

In this order:

1. a provider for OpenAI-compatible Chat Completions endpoints,
   `roomkit-providers-openai`: written, not released. It answers a request
   whole or as a stream and lists its endpoint's models, replaying the cases
   captured on RoomKit's Python provider;
2. a Python package holding the same implementation behind a native
   extension, `roomkit-providers`: written, not released, for Linux x86_64.
   A provider is a RoomKit `AIProvider`, and what translates between RoomKit
   and the contract is written once for every provider ([Python](#python));
3. a provider for Anthropic's Messages API,
   `roomkit-providers-anthropic`: written, not released, in Rust and in the
   Python package, which it joined with no change to what every provider
   shares there. It replays the cases captured on RoomKit's Python
   provider, and is written on the HTTP client the OpenAI provider is
   written on, `roomkit-providers-client`: a provider crate holds its wire
   and nothing else;
4. the other providers of language models, then the other kinds of
   provider;
5. RoomKit's implementations, in Python and in Rust, depending on the
   contract and on no vendor.

Items 5 and 6 are not started.

## Python

The Python package holds the same providers behind a native extension,
OpenAI's and Anthropic's, for RoomKit on CPython 3.12 or newer. A provider
is a RoomKit
`AIProvider`: it takes an `AIContext`, gives RoomKit's models and raises
RoomKit's `ProviderError`, and an AI channel takes it as it takes RoomKit's
own:

```python
from roomkit import AIChannel
from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

config = OpenAIConfig(
    api_key="placeholder-not-a-key",
    model="local-model",
    base_url="http://localhost:11434/v1",
)
assistant = AIChannel("assistant", provider=OpenAI(config))
```

The providers are filed by kind, then by vendor, each class named for its
vendor. Nothing is published: `make python-wheel` builds the wheel from this
repository. The [Python guide](guide/python.md) says how to install it, how
to switch a RoomKit agent to it, what the provider holds, how a call is
cancelled and how it fails, and where it differs from RoomKit's own
provider.

## Layout

| Path | Contents |
| --- | --- |
| `crates/roomkit-providers-http` | Outbound URL validation (`validate_public_url`), and the HTTP support the providers share: bounded readers of server-sent events and of JSON lines, and the TLS configuration of their clients, which trusts the certificate authorities the environment or the system names |
| `crates/roomkit-providers-client` | The HTTP client a provider is built on, written once for every provider: a request and its tries, the proxies it reads from the environment, the bounds of an answer and of a stream, the base URLs refused when a provider is built, the one exit of failures, and, for a provider's tests, the replay of its recorded wire cases against a local fake |
| `crates/roomkit-providers-openai` | The OpenAI provider (`OpenAi`): whole answers, streams and the model listing, its configuration (`OpenAiConfig`), the wire cases it replays, and two examples, against a local server and behind a local proxy; and the providers of eleven vendors on the same wire, `DeepSeek`, `Kimi`, `MiniMax`, `Xai`, `Cerebras`, `Qwen`, `Meta`, `OpenRouter`, `LiteLlm`, `AzureAi` and `Vllm`, each with its configuration |
| `crates/roomkit-providers-anthropic` | The Anthropic provider (`Anthropic`): whole answers, streams and the model listing on the Messages API, its configuration (`AnthropicConfig`), the wire cases it replays, and an example against a local server |
| `crates/roomkit-providers-gemini` | The Gemini provider (`Gemini`): whole answers, streams and the model listing on Google's Developer API, its configuration (`GeminiConfig`), and the wire cases it replays |
| `crates/roomkit-providers-mistral` | The Mistral provider (`Mistral`): whole answers, streams and the model listing on Mistral's chat completions, its configuration (`MistralConfig`), and the wire cases it replays |
| `crates/roomkit-providers-ollama` | The Ollama provider (`Ollama`): whole answers, streams of JSON lines and the listing of the models a server holds, on Ollama's native API, its configuration (`OllamaConfig`), and the wire cases it replays |
| `crates/roomkit-providers-polargrid` | The PolarGrid provider (`PolarGrid`): whole answers, streams and the model listing on the chat completions of an edge, pinned or picked by the autorouter, its configuration (`PolarGridConfig`), and the wire cases it replays |
| `crates/roomkit-providers-redis` | RoomKit's turn backend across processes, on Redis (`RedisTurnBackend`): the broker and the registry of the core's distributed backend (`RedisTurnBroker`, `RedisTurnRegistry`), with the keys, the scripts and the messages of RoomKit's Python backend. Rust only. Its functional tests, its interop with RoomKit's Python and its example of two processes run on a Redis server (`make redis-test`) |
| `bindings/python` | The native extension of the Python package (PyO3), a workspace of its own: the Rust workspace builds without a Python interpreter. One class holds any provider of the contract, and the entries Python calls are written once for every vendor (`src/vendor.rs`); `src/vendors` holds each provider's own part, its configuration and how it is built |
| `bindings/python-forms` | The JSON forms of RoomKit's Python models, read into and written from the types of RoomKit's Rust core: what crosses to Python, and what a recorded case holds. The conversion and nothing else |
| `python/roomkit_providers` | The Python package, by kind then by vendor: `roomkit_providers.llm` holds what every provider of a language model shares, a RoomKit `AIProvider` over a provider of the contract and what every configuration does, `roomkit_providers.llm.openai` holds `OpenAI` and `OpenAIConfig`, `roomkit_providers.llm.anthropic` holds `Anthropic` and `AnthropicConfig`, `roomkit_providers.llm.gemini` holds `Gemini` and `GeminiConfig`, `roomkit_providers.llm.mistral` holds `Mistral` and `MistralConfig`, `roomkit_providers.llm.ollama` holds `Ollama` and `OllamaConfig`, `roomkit_providers.llm.polargrid` holds `PolarGrid` and `PolarGridConfig`, `roomkit_providers.llm.deepseek`, `.kimi`, `.minimax`, `.xai`, `.cerebras`, `.qwen`, `.meta`, `.openrouter`, `.litellm`, `.azure` and `.vllm` hold `DeepSeek`, `Kimi`, `MiniMax`, `XAI`, `Cerebras`, `Qwen`, `Meta`, `OpenRouter`, `LiteLLM`, `AzureAI`, `VLLM` and their configurations, a class statement each |
| `tests/python` | The Python tests: what every provider shares, against a scripted provider of no vendor; the criterion that adding a provider changes none of it; the replay of each provider's recorded wire cases through its class; a RoomKit agent over each; cancellation, event loops and threads, the interpreter's exit; and the snapshot of the public API |
| `examples/local_server.py` | A Python program that asks a stand-in server on a local port, whole and streamed |
| `examples/roomkit_agent.py` | A RoomKit agent over the provider: it answers a message, calls a tool and streams its answer, against a stand-in server on a local port |
| `examples/anthropic_agent.py` | The same agent over `Anthropic`: the model reasons, calls a tool, and the round goes back with its signed reasoning |
| `rust-examples` | Fourteen RoomKit agents on these providers, in Rust, each a program that checks what it shows against a local fake of its provider's wire: a first agent on `OpenAi` and on `Anthropic`, a response schema, a fallback model, a streamed answer, tools and their search, history and intent, skills, MCP tools, and an agent served over WebSocket with its page. `shared/` holds what they share: the fake of the OpenAI wire, the server and its WebSocket channel. They come from RoomKit's Rust core, which holds no provider |
| `benchmarks/crossing.py` | What a turn costs through a provider of the package (`--provider openai` or `anthropic`), and through RoomKit's own Python provider where its SDK is installed |
| `benchmarks/footprint.py`, `benchmarks/fork_children.py` | The threads and the connections a busy provider holds, and how often a child forked under load hangs |
| `guide/python.md` | The Python guide |
| `scripts/check-python.py` | Builds a wheel and a source distribution, installs each beside RoomKit and runs the Python tests and the examples on it (`scripts/artifact_files.py` holds what a distribution may contain, `scripts/native-notices.py` the notices of the extension's dependencies, `scripts/api-snapshot.py` rewrites the API snapshot, `scripts/roomkit_env.py` names the RoomKit of the environment) |
| `scripts/no-network.sh` | Runs a command where no network is reachable but loopback, without the proxy and certificate variables of the machine: the tests, the examples and the benches go through it. Where the machine lets a user make no such place, it runs nothing and fails, unless `ROOMKIT_PROVIDERS_ALLOW_NETWORK=1` is set (`scripts/no-network-test.sh` checks it) |
| `scripts/generate_openai_catalogue.py` | Writes the model catalogues of RoomKit's Python for OpenAI, DeepSeek, Kimi, MiniMax, xAI, Cerebras, Qwen, Meta and OpenRouter, which each provider reads its models' rules from |
| `scripts/generate_python_forms.py` | Writes what RoomKit's Python gives, which the contract's tests compare with: the JSON forms of its models and the decisions of its call rules |
| `scripts/generate_python_public_urls.py` | Writes what RoomKit's Python `validate_public_url` decides of a table of URLs, which the HTTP crate's tests replay |
| `scripts/generate_python_proxy_cases.py` | Writes which proxy the client of RoomKit's Python providers takes for a table of environments, which the client crate's tests replay |
| `scripts/generate_openai_config_forms.py` | Writes the JSON forms of RoomKit's Python `OpenAIConfig`, which the OpenAI crate's tests compare with |
| `scripts/generate_openai_chat_cases.py` | Runs RoomKit's Python OpenAI provider against a fake endpoint and writes what it sends and reads, case by case (`scripts/openai_chat_cases.py` holds the cases) |
| `scripts/generate_anthropic_catalogue.py` | Writes Anthropic's model catalogue of RoomKit's Python, which the provider reads its models' rules from |
| `scripts/generate_anthropic_config_forms.py` | Writes the JSON forms of RoomKit's Python `AnthropicConfig`, which the Anthropic crate's tests compare with |
| `scripts/generate_anthropic_messages_cases.py` | Runs RoomKit's Python Anthropic provider against a fake endpoint and writes what it sends and reads, case by case (`scripts/anthropic_messages_cases.py` holds the cases) |
| `scripts/wire_capture.py` | What the two scripts that capture a provider's wire cases share: what a case is, how the fake answers, how an exchange, an outcome and a failure are written |
| `scripts/generate_printable_ranges.py` | Writes the table of the characters Python's `repr` writes as they are |
| `scripts/generate_outside_text_tables.py` | Writes the tables the contract reads a text from outside with: what each character reads as in a tag, the classes of characters, and canonical composition |
| `scripts/generate_python_outside_text.py` | Writes what RoomKit's Python `fence` and `identifier` make of a table of texts and names, which the contract's tests compare with |
| `scripts/unicode_notice.py` | The notice of the Unicode License V3, which the scripts that read Python's Unicode data write as their crate's `LICENSE-UNICODE` |

## Development

```sh
make all            # formatting, lints, tests, examples, docs and dependency policy
make msrv           # the minimum supported Rust version
make python-check   # the Python package: lints, a wheel and a source distribution, tests
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the environment, the checks and
the project's rules, and [SECURITY.md](SECURITY.md) to report a
vulnerability.

## License

[MIT](LICENSE).

`roomkit-providers-http` is `MIT AND Unicode-3.0`: it holds a list derived
from Unicode data, under the Unicode License V3, which the crate ships as
its [`LICENSE-UNICODE`](crates/roomkit-providers-http/LICENSE-UNICODE).

The Python package's own code is MIT. Its wheel links the crates of this repository,
RoomKit's Rust core and the crates they depend on, a few of which are under no license it could take
them under in place of their own: the wheel is
`MIT AND Unicode-3.0 AND Apache-2.0 AND BSD-3-Clause AND CDLA-Permissive-2.0 AND ISC AND Zlib`,
and ships the notices of every crate its extension is built from
(`roomkit_providers/licenses/native_dependencies.txt`).
