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
> the model listing. A Python package holds that provider behind a native
> extension, as a RoomKit provider. Nothing is released. The
> [changelog](CHANGELOG.md) says what exists; the rest of this page says
> where the project is going.

## Goals

- **One implementation per provider.** A provider's wire format, streaming,
  retries and failures are written once, in Rust.
- **A provider-neutral contract.** A request, its messages and tools, the
  response, the events of a streamed answer, typed failures and model
  catalogues are defined apart from any vendor.
- **Tied to no core.** The Rust contract and the Rust providers need
  neither RoomKit nor an agent framework, and RoomKit's cores are to depend
  on the contract, never on a vendor. The Python package is RoomKit's: a
  provider there is a RoomKit provider.
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
3. a Python package holding the same implementation behind a native
   extension, `roomkit-providers`: written, not released, for Linux x86_64.
   A provider is a RoomKit `AIProvider`, and what translates between RoomKit
   and the contract is written once for every provider ([Python](#python));
4. the other providers of language models, then the other kinds of
   provider;
5. RoomKit's implementations, in Python and in Rust, depending on the
   contract and on no vendor.

Items 4 and 5 are not started.

## Python

The Python package holds the same OpenAI provider behind a native extension,
for RoomKit on CPython 3.12 or newer. The provider is a RoomKit
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
| `crates/roomkit-providers-llm` | The contract crate: the provider trait, the request, the response and its stream events, the response schema, the replay of a round, the provider failure and the model catalogue, with a scripted provider for tests |
| `crates/roomkit-providers-http` | Outbound URL validation (`validate_public_url`), and the HTTP support the providers share: a bounded reader of server-sent events and the TLS configuration of their clients |
| `crates/roomkit-providers-client` | The HTTP client a provider is built on, written once for every provider: a request and its tries, the bounds of an answer and of a stream, the base URLs refused when a provider is built, the one exit of failures, and, for a provider's tests, the replay of its recorded wire cases against a local fake |
| `crates/roomkit-providers-openai` | The OpenAI provider (`OpenAi`): whole answers, streams and the model listing, its configuration (`OpenAiConfig`), the wire cases it replays, and an example against a local server |
| `bindings/python` | The native extension of the Python package (PyO3), a workspace of its own: the Rust workspace builds without a Python interpreter. One class holds any provider of the contract; `src/vendors` holds each provider's own part |
| `python/roomkit_providers` | The Python package, by kind then by vendor: `roomkit_providers.llm` holds what every provider of a language model shares, a RoomKit `AIProvider` over a provider of the contract, and `roomkit_providers.llm.openai` holds `OpenAI` and `OpenAIConfig` |
| `tests/python` | The Python tests: what every provider shares, against a scripted provider of no vendor; the replay of the recorded wire cases through `OpenAI`; a RoomKit agent over it; cancellation, event loops and threads, the interpreter's exit; and the snapshot of the public API |
| `examples/local_server.py` | A Python program that asks a stand-in server on a local port, whole and streamed |
| `examples/roomkit_agent.py` | A RoomKit agent over the provider: it answers a message, calls a tool and streams its answer, against a stand-in server on a local port |
| `benchmarks/crossing.py` | What a turn costs through the package's provider, and through RoomKit's own Python provider where its SDK is installed |
| `benchmarks/footprint.py`, `benchmarks/fork_children.py` | The threads and the connections a busy provider holds, and how often a child forked under load hangs |
| `guide/python.md` | The Python guide |
| `scripts/check-python.py` | Builds a wheel and a source distribution, installs each beside RoomKit and runs the Python tests and the examples on it (`scripts/artifact_files.py` holds what a distribution may contain, `scripts/native-notices.py` the notices of the extension's dependencies, `scripts/api-snapshot.py` rewrites the API snapshot, `scripts/roomkit_env.py` names the RoomKit of the environment) |
| `scripts/no-network.sh` | Runs a command where no network is reachable but loopback: the tests, the examples and the benches go through it |
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
make python-check   # the Python package: lints, a wheel and a source distribution, tests
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

The Python package's own code is MIT. Its wheel links both crates and the
crates they depend on, a few of which are under no license it could take
them under in place of their own: the wheel is
`MIT AND Unicode-3.0 AND Apache-2.0 AND BSD-3-Clause AND CDLA-Permissive-2.0 AND ISC AND Zlib`,
and ships the notices of every crate its extension is built from
(`roomkit_providers/licenses/native_dependencies.txt`).
