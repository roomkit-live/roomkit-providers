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
> the providers will share, and the configuration of the OpenAI provider. No
> provider implements the contract yet beside a scripted one for tests, and
> nothing is released. The
> [changelog](CHANGELOG.md) says what exists; the rest of this page says
> where the project is going.

## Goals

- **One implementation per provider.** A provider's wire format, streaming,
  retries and failures are written once, in Rust.
- **A provider-neutral contract.** A request, its messages and tools, the
  response, the events of a streamed answer, typed failures and model
  catalogues are defined apart from any vendor.
- **Usable on its own.** The contract and the providers need neither RoomKit
  nor an agent framework. RoomKit integrations are optional adapters.
- **The host stays in control.** Endpoints, credentials, timeouts and limits
  are explicit host configuration. A credential never appears in an error, a
  log or a result.

Section numbers in the code and its documentation (`RFC §6.7`) refer to the
[RoomKit RFC](https://github.com/roomkit-live/roomkit-specs).

## Planned

In this order:

1. the contract crate, `roomkit-providers-core`: written, not released;
2. a provider for OpenAI-compatible Chat Completions endpoints,
   `roomkit-providers-openai`. The HTTP support it builds on,
   `roomkit-providers-http`, is written, with the provider's configuration
   and the cases it will replay, captured on RoomKit's Python provider; the
   provider itself is not written;
3. a Python package calling the same implementation through a native
   extension;
4. adapters for RoomKit, in Rust and in Python.

Items 3 and 4 are not started.

## Layout

| Path | Contents |
| --- | --- |
| `crates/roomkit-providers-core` | The contract crate: the provider trait, the request, the response and its stream events, the response schema, the replay of a round, the provider failure and the model catalogue, with a scripted provider for tests |
| `crates/roomkit-providers-http` | Outbound URL validation (`validate_public_url`), and the HTTP support the providers share: a bounded reader of server-sent events and the TLS configuration of their clients |
| `crates/roomkit-providers-openai` | The OpenAI provider: its configuration (`OpenAiConfig`) today, and the wire cases its Chat Completions client will replay |
| `scripts/generate_python_forms.py` | Writes what RoomKit's Python gives, which the contract's tests compare with: the JSON forms of its models and the decisions of its call rules |
| `scripts/generate_python_public_urls.py` | Writes what RoomKit's Python `validate_public_url` decides of a table of URLs, which the HTTP crate's tests replay |
| `scripts/generate_openai_config_forms.py` | Writes the JSON forms of RoomKit's Python `OpenAIConfig`, which the OpenAI crate's tests compare with |
| `scripts/generate_openai_chat_cases.py` | Runs RoomKit's Python OpenAI provider against a fake endpoint and writes what it sends and reads, case by case (`scripts/openai_chat_cases.py` holds the cases) |
| `scripts/generate_printable_ranges.py` | Writes the table of the characters Python's `repr` writes as they are |
| `scripts/unicode_notice.py` | The notice of the Unicode License V3, which the two scripts that read Python's Unicode data write as their crate's `LICENSE-UNICODE` |

## Development

```sh
make all    # formatting, lints, tests, docs and dependency policy
make msrv   # the minimum supported Rust version
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the environment, the checks and
the project's rules, and [SECURITY.md](SECURITY.md) to report a
vulnerability.

## License

[MIT](LICENSE).

`roomkit-providers-core` and `roomkit-providers-http` are
`MIT AND Unicode-3.0`: the first holds a table and the second a list derived
from Unicode data, under the Unicode License V3, which each crate ships as
its `LICENSE-UNICODE`
([core](crates/roomkit-providers-core/LICENSE-UNICODE),
[http](crates/roomkit-providers-http/LICENSE-UNICODE)).
