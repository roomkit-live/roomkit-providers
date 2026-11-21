# RoomKit Providers

**Model providers for AI applications, written in Rust.**

[![CI](https://github.com/roomkit-live/roomkit-providers/actions/workflows/ci.yml/badge.svg)](https://github.com/roomkit-live/roomkit-providers/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/roomkit-live/roomkit-providers/blob/main/LICENSE)
[![MSRV: 1.94](https://img.shields.io/badge/MSRV-1.94-orange.svg)](https://github.com/roomkit-live/roomkit-providers/blob/main/Cargo.toml)

RoomKit Providers is where the clients of model services live for the
[RoomKit](https://github.com/roomkit-live/roomkit) ecosystem: one
implementation per service, written once in Rust, behind a provider-neutral
contract.

> **Early development.** The contract crate is written: what a provider
> implements, what it is asked and what it answers. So is the HTTP support
> the providers share, and the OpenAI provider: whole answers, streams and
> the model listing. A Python package calls that provider through a native
> extension, on its own or as a RoomKit provider through its adapter.
> Nothing is released. The [changelog](CHANGELOG.md) says what exists; the
> rest of this page says where the project is going.

## Goals

- **One implementation per provider.** A provider's wire format, streaming,
  retries and failures are written once, in Rust.
- **A provider-neutral contract.** A request, its messages and tools, the
  response, the events of a streamed answer, typed failures and model
  catalogues are defined apart from any vendor.
- **Usable on its own.** The contract and the providers need neither RoomKit
  nor an agent framework. RoomKit integrations are optional adapters.
- **The host stays in control.** Endpoints, credentials, timeouts and limits
  are explicit host configuration. A provider writes no credential into a
  log, a `Debug` form or a result, and removes from an error's text a
  credential of 8 characters or more that a server sends back, as it is or
  as the error's own code writes it again; its README states the limits.

Section numbers in the code and its documentation (`RFC §6.7`) refer to the
[RoomKit RFC](https://github.com/roomkit-live/roomkit-specs).

## Planned

In this order:

1. the contract crate, `roomkit-providers-llm`: written, not released;
2. a provider for OpenAI-compatible Chat Completions endpoints,
   `roomkit-providers-openai`: written, not released. It answers a request
   whole or as a stream and lists its endpoint's models, replaying the cases
   captured on RoomKit's Python provider;
3. a Python package calling the same implementation through a native
   extension, `roomkit-providers`: written, not released, for Linux x86_64
   ([Python](#python));
4. adapters for RoomKit, in Rust and in Python. The Python one is written,
   not released: `roomkit_providers.roomkit`. The Rust one is not started.

## Python

The Python package holds the same OpenAI provider behind a native extension,
for asynchronous programs on CPython 3.12 or newer. It has no Python
dependency. A request and its answer are plain dicts in the contract's JSON
form:

```python
from roomkit_providers.openai import OpenAIAIProvider, OpenAIConfig

config = OpenAIConfig(
    api_key="placeholder-not-a-key",
    model="local-model",
    base_url="http://localhost:11434/v1",
)
provider = OpenAIAIProvider(config)


async def ask() -> str:
    request = {"messages": [{"role": "user", "content": "Hello"}]}
    return (await provider.generate(request))["content"]
```

In a RoomKit agent, the adapter takes the place of RoomKit's own OpenAI
provider: it is an `AIProvider`, takes RoomKit's `OpenAIConfig` and an
`AIContext`, gives RoomKit's models and raises RoomKit's `ProviderError`.
It needs RoomKit, which the `roomkit` extra asks for:

```python
from roomkit import AIChannel
from roomkit.providers.openai import OpenAIConfig
from roomkit_providers.roomkit import OpenAIAIProvider

assistant = AIChannel("assistant", provider=OpenAIAIProvider(OpenAIConfig(...)))
```

Nothing is published: `make python-wheel` builds the wheel from this
repository. The [Python guide](guide/python.md) says how to install it, what
the package holds, how a call is cancelled and how it fails, how to switch
a RoomKit agent to the adapter, and where each differs from RoomKit's
Python provider.

## Layout

| Path | Contents |
| --- | --- |
| `crates/roomkit-providers-llm` | The contract crate: the provider trait, the request, the response and its stream events, the response schema, the replay of a round, the provider failure and the model catalogue, with a scripted provider for tests |
| `crates/roomkit-providers-http` | Outbound URL validation (`validate_public_url`), and the HTTP support the providers share: a bounded reader of server-sent events and the TLS configuration of their clients |
| `crates/roomkit-providers-openai` | The OpenAI provider (`OpenAi`): whole answers, streams and the model listing, its configuration (`OpenAiConfig`), the wire cases it replays, and an example against a local server |
| `bindings/python` | The native extension of the Python package (PyO3), a workspace of its own: the Rust workspace builds without a Python interpreter |
| `python/roomkit_providers` | The Python package: `roomkit_providers.openai` (`OpenAIAIProvider`, `OpenAIConfig`), its errors and the typed dicts of the contract's JSON forms |
| `python/roomkit_providers/roomkit` | The RoomKit adapter: `OpenAIAIProvider` as a RoomKit `AIProvider`, importable where RoomKit is installed |
| `tests/python` | The Python tests: the replay of the recorded wire cases through the package, cancellation, event loops and threads, the interpreter's exit, and the snapshot of the public API |
| `tests/python/roomkit` | The adapter's tests, skipped where RoomKit is not installed: the replay through the adapter, a RoomKit agent over it, the first-token metric, and the adapter's API snapshot |
| `examples/local_server.py` | A Python program that asks a stand-in server on a local port, whole and streamed |
| `examples/roomkit_agent.py` | A RoomKit agent over the adapter: it answers a message, calls a tool and streams its answer, against a stand-in server on a local port |
| `benchmarks/crossing.py` | What a turn costs through the Python package and through its adapter, and through RoomKit's Python provider when it is installed |
| `guide/python.md` | The Python guide |
| `scripts/check-python.py` | Builds a wheel and a source distribution, installs each alone and runs the Python tests and the example on it; with `--roomkit`, installs the wheel with its `roomkit` extra and runs the adapter's tests and example (`scripts/artifact_files.py` holds what a distribution may contain, `scripts/native-notices.py` the notices of the extension's dependencies, `scripts/api-snapshot.py` rewrites the API snapshots, `scripts/roomkit_env.py` names the RoomKit of the environment) |
| `scripts/generate_openai_catalogue.py` | Writes OpenAI's model catalogue of RoomKit's Python, which the provider reads its models' rules from |
| `scripts/generate_python_forms.py` | Writes what RoomKit's Python gives, which the contract's tests compare with: the JSON forms of its models and the decisions of its call rules |
| `scripts/generate_python_public_urls.py` | Writes what RoomKit's Python `validate_public_url` decides of a table of URLs, which the HTTP crate's tests replay |
| `scripts/generate_openai_config_forms.py` | Writes the JSON forms of RoomKit's Python `OpenAIConfig`, which the OpenAI crate's tests compare with |
| `scripts/generate_openai_chat_cases.py` | Runs RoomKit's Python OpenAI provider against a fake endpoint and writes what it sends and reads, case by case (`scripts/openai_chat_cases.py` holds the cases) |
| `scripts/generate_printable_ranges.py` | Writes the table of the characters Python's `repr` writes as they are |
| `scripts/unicode_notice.py` | The notice of the Unicode License V3, which the two scripts that read Python's Unicode data write as their crate's `LICENSE-UNICODE` |

## Development

```sh
make all            # formatting, lints, tests, examples, docs and dependency policy
make msrv           # the minimum supported Rust version
make python-check   # the Python package: lints, a wheel and a source distribution, tests,
                    # and the RoomKit adapter where RoomKit is installed
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the environment, the checks and
the project's rules, and [SECURITY.md](SECURITY.md) to report a
vulnerability.

## License

[MIT](LICENSE).

`roomkit-providers-llm` and `roomkit-providers-http` are
`MIT AND Unicode-3.0`: the first holds a table and the second a list derived
from Unicode data, under the Unicode License V3, which each crate ships as
its `LICENSE-UNICODE`
([llm](crates/roomkit-providers-llm/LICENSE-UNICODE),
[http](crates/roomkit-providers-http/LICENSE-UNICODE)).

The Python package links both, so it is `MIT AND Unicode-3.0` too, and ships
the notices of the crates its extension is built from
(`roomkit_providers/licenses/native_dependencies.txt`).
