# Contributing

Start with the [project overview](README.md). Keep public behavior, examples
and documentation consistent with the implementation, and keep what exists
distinct from what is planned.

## Development environment

Rustup selects the toolchain pinned in `rust-toolchain.toml`; the MSRV is 1.94.
Install cargo-deny for dependency checks:

```sh
cargo install cargo-deny --locked
make hooks
```

`make hooks` enables the optional commit-message hook.

## Validation gates

| Command | Scope |
| --- | --- |
| `make fmt` | Format the workspace |
| `make all` | Formatting, Clippy, tests, rustdoc, dependency policy and the core dependency allow list |
| `make msrv-toolchain msrv` | Install and check the minimum Rust toolchain |

Run relevant checks during development, then `make all`. Run the MSRV check
after adding dependencies or using newer standard-library APIs.

With populated Cargo caches and an advisory database, an offline check can use
`CARGO_NET_OFFLINE=true make DENY_FLAGS=--offline all`. This checks cached
advisories only; CI refreshes them. Required checkers must not be silently
skipped.

## Contracts and tests

Test observable behavior: what a caller reads and what goes on the wire,
especially boundaries, failures, cancellation and cleanup.

- No test reaches a vendor's service. A provider is tested against a local
  fake of the wire it speaks.
- No key belongs in a fixture, a test or a command. Default checks require no
  external service and no credentials.
- A bug fix brings the test that failed before it.

## One public API

The project is meant to be used from Rust and from Python, and by RoomKit in
both. It has one public API: the same names, parameters, defaults and
behavior everywhere, apart from what a language forces, such as Rust's casing
of an acronym, a struct of named fields for keyword arguments or a `Result`
for a raised exception.

- A type that RoomKit's Python API already names keeps that name here.
- Any other difference needs a maintainer's decision before it is written.

## JSON forms

Every contract type reads and writes the JSON its model in RoomKit's Python
implementation gives with `model_dump(mode="json")`: no form is specific to
one language. The proof is a fixture written by RoomKit's Python,
`crates/roomkit-providers-core/tests/fixtures/python_forms.json`, which the
`python_forms` test reads into the Rust types and writes back.

- `make all` and CI only read the fixture: they need no Python.
- `make python-forms` rewrites it with `scripts/generate_python_forms.py`. It
  needs an interpreter that has the `roomkit` package installed, named by
  `PYTHON` (`python3` by default):

  ```sh
  make python-forms PYTHON=path/to/venv/bin/python
  ```

- A new contract type or field brings its examples to the script, and the
  regenerated fixture is committed with it. The fixture records the RoomKit
  version it was written from.

## Implementation boundaries

- Keep `roomkit-providers-core` free of I/O, async runtimes, Python and
  RoomKit dependencies. `make check-core-deps` holds its allow list; update
  the list in the commit that changes the core's dependencies.
- Put dependency versions in `workspace.dependencies`; consuming crates enable
  only the features they need.
- The host sets endpoints, credentials, timeouts and limits explicitly. A
  provider reads no environment variable and no global state for them.
- A credential never appears in an error, a log, a debug representation or a
  result.
- Document public items; return typed failures instead of panicking on what a
  caller or a service sends.
- Prefer `#[expect(lint, reason = "...")]` for a justified lint exception.

## Documentation

Code, comments, examples and public documentation use English and generic
data. Keep local files out of version control with your checkout's
`.git/info/exclude` and do not force-add ignored material.

## Commits

Use `type(scope): summary`, at most 72 characters, for example
`feat(core): add the request contract`. The hook and CI share the same
checker. Record user-visible changes under `Unreleased` in
[CHANGELOG.md](CHANGELOG.md).
