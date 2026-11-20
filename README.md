# RoomKit Providers

**Model providers for AI applications, written in Rust.**

[![CI](https://github.com/roomkit-live/roomkit-providers/actions/workflows/ci.yml/badge.svg)](https://github.com/roomkit-live/roomkit-providers/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/roomkit-live/roomkit-providers/blob/main/LICENSE)
[![MSRV: 1.94](https://img.shields.io/badge/MSRV-1.94-orange.svg)](https://github.com/roomkit-live/roomkit-providers/blob/main/Cargo.toml)

RoomKit Providers is where the clients of model services live for the
[RoomKit](https://github.com/roomkit-live/roomkit) ecosystem: one
implementation per service, written once in Rust, behind a provider-neutral
contract.

> **Early development.** The contract crate defines the request a provider is
> given and its tools; no provider exists, so nothing is usable yet. The
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

## Planned

In this order:

1. the contract crate, `roomkit-providers-core`. Its request, message and
   tool types exist, with their JSON forms; the response, the events of a
   streamed answer, the typed failures and the model catalogues do not yet;
2. a provider for OpenAI-compatible Chat Completions endpoints;
3. a Python package calling the same implementation through a native
   extension;
4. adapters for RoomKit, in Rust and in Python.

Items 2 to 4 are not started.

## Layout

| Path | Contents |
| --- | --- |
| `crates/roomkit-providers-core` | The contract crate: the request, its messages and its tools, with their JSON forms |
| `scripts/generate_python_forms.py` | Writes the examples of RoomKit's Python JSON forms that the contract's tests read |

## Development

```sh
make all    # formatting, lints, tests, docs and dependency policy
make msrv   # the minimum supported Rust version
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the environment, the checks and
the project's rules, and [SECURITY.md](SECURITY.md) to report a
vulnerability.

## License

[MIT](LICENSE)
