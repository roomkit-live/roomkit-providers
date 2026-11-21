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
- No test asks a name of the network. A test of `validate_public_url` gives
  its own answers for the names it resolves, or uses a URL that needs none.
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

Every contract type that is a model in RoomKit's Python implementation reads
and writes the JSON that model gives with `model_dump(mode="json")`: no form
is specific to one language. The proof is a fixture written by RoomKit's
Python,
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
- A public type never derives `Deserialize`: a derived reader also takes a
  list of a model's values for the model, and an object or a number for a
  name. A model is read as a private form through `read_as_object!`, a name
  through `read_name`, a union of models through `read_tagged`
  (`src/strict/`), and joins the lists of `tests/strict_input.rs`.
- The same script writes `python_call_rules.json`, what RoomKit's Python
  decides of a call's arguments, which the `python_call_rules` test compares
  the Rust rules with.

The words of a refusal are Python's too, which quote a value with `repr`.
`repr` escapes the characters `str.isprintable()` refuses, so the crate
carries that table, generated from Python's Unicode data rather than read
from Rust's standard library:

```sh
make printable-table PYTHON=path/to/venv/bin/python
```

Run it with the Python version RoomKit is tested with; the table's header
records the Python and Unicode versions it was written from, and carries
the notice of the Unicode data it is derived from. The same command writes
that notice as `crates/roomkit-providers-core/LICENSE-UNICODE`, the license
file the crate ships beside its own: the crate is `MIT AND Unicode-3.0`.

`roomkit-providers-http` is checked the same way: `make python-public-urls`
rewrites `crates/roomkit-providers-http/tests/fixtures/python_public_urls.json`,
what RoomKit's Python `validate_public_url` decides of a table of URLs and
addresses, with the same `PYTHON`. The script answers its own lookups from a
table the fixture carries, and the test resolves from that table. A case
where the crate does not decide as Python is named in the test, with the
reason, never left out of the fixture. The fixture also carries the
characters that Python's Unicode data turns into a delimiter of a URL, which
the crate lists: the same command writes the notice of that data as
`crates/roomkit-providers-http/LICENSE-UNICODE`, and the crate is
`MIT AND Unicode-3.0` too.

`roomkit-providers-openai` has two fixtures, written with the same `PYTHON`,
which needs RoomKit's `openai` extra for the second:

```sh
make python-openai-config
make python-openai-chat-cases
```

- `python_config_forms.json` holds the JSON forms of Python's `OpenAIConfig`:
  what it dumps of a table of keyword arguments, the inputs it refuses, the
  inputs it converts, which `OpenAiConfig` refuses, and whether each of a
  table of base URLs is OpenAI's own endpoint.
- `python_chat_cases.json` holds the wire cases: for each, a configuration,
  a request, the HTTP requests Python's `OpenAIAIProvider` sent to a fake of
  the endpoint with what the fake answered, and what Python returned. The
  cases are listed in `scripts/openai_chat_cases.py`, each with the kinds of
  behaviour it shows; the `python_chat_cases` test requires every kind, so a
  case is added there with its kind, and a kind is never dropped. Nothing
  that changes from a run to the next is recorded: the script stops the
  SDK's clock, counts the ids Python mints instead of drawing them, and
  leaves out the headers of the SDK. It must write the same bytes twice.
  Where the openai SDK decides a behaviour, not RoomKit, the case says so
  under `sdk`: the Rust provider has no SDK under it.

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
