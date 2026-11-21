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

The Python package also needs CPython 3.12 or newer and
[uv](https://docs.astral.sh/uv/), which runs maturin and ruff at a version
within the range the `Makefile` names. Name a local installation in their
place, for instance one in a virtual environment of the repository (`.venv`
is ignored by git):

```sh
uv venv --python 3.12 .venv
uv pip install --python .venv/bin/python 'maturin>=1.15,<2' 'ruff>=0.14,<1'
make python-check PYTHON=.venv/bin/python MATURIN=$PWD/.venv/bin/maturin RUFF=$PWD/.venv/bin/ruff
```

The RoomKit adapter is checked where `PYTHON`'s environment has RoomKit, in
the version the `roomkit` extra of `pyproject.toml` names. Install it there
from a checkout of RoomKit, not editable:

```sh
uv pip install --python .venv/bin/python ../roomkit
```

## Validation gates

| Command | Scope |
| --- | --- |
| `make fmt` | Format the workspace |
| `make all` | Formatting, Clippy, tests, the examples, rustdoc, dependency policy and the contract crate's dependency allow list |
| `make msrv-toolchain msrv` | Install and check the minimum Rust toolchain |
| `make python-check` | The Python package: the binding's formatting, Clippy, rustdoc and dependency policy, the Python lint, then a wheel and a source distribution, each installed alone and tested; then `make python-roomkit-check` where `PYTHON`'s environment has RoomKit |
| `make python-roomkit-check` | The RoomKit adapter: a wheel installed with its `roomkit` extra beside the RoomKit of `PYTHON`'s environment, the adapter's tests and its example |
| `make python-msrv` | Check the binding on the minimum Rust toolchain |
| `make python-wheel` | Build the optimized wheel into `dist/` |
| `make api-snapshot` | Rewrite the snapshot of the Python public API from a freshly built wheel |

Run relevant checks during development, then `make all`. Run the MSRV check
after adding dependencies or using newer standard-library APIs. For a change
to the Python package, the binding or what they are built from, also run
`make python-check`, and `make python-msrv` after a change of dependencies.

`make all` needs no Python interpreter: the binding is a workspace of its
own (`bindings/python`), which the Rust workspace does not build.

With populated Cargo caches and an advisory database, an offline check can use
`CARGO_NET_OFFLINE=true make DENY_FLAGS=--offline all`. This checks cached
advisories only; CI refreshes them. Required checkers must not be silently
skipped.

## Contracts and tests

Test observable behavior: what a caller reads and what goes on the wire,
especially boundaries, failures, cancellation and cleanup.

- No test reaches a vendor's service. A provider is tested against a local
  fake of the wire it speaks.
- An example runs with no service and no key: it starts the server it talks
  to on a local port, checks what it shows and fails otherwise. `make all`
  runs each example (`make examples`), with the proxy variables set to a
  local port nothing listens on: a provider reads none of them.
- A test does not wait on a timeout, and asserts nothing about how fast an
  answer comes. It waits on what the local fake observed, then lets the
  timeout's time pass at once (the tests' `let_pass`).
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
`crates/roomkit-providers-llm/tests/fixtures/python_forms.json`, which the
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
that notice as `crates/roomkit-providers-llm/LICENSE-UNICODE`, the license
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

`roomkit-providers-openai` has two fixtures and a catalogue, written with
the same `PYTHON`, which needs RoomKit's `openai` extra for the second:

```sh
make python-openai-config
make python-openai-chat-cases
make python-openai-catalogue
```

The catalogue (`catalog/models.json`) is OpenAI's models as RoomKit's Python
lists them: the provider reads from it what OpenAI's own endpoint takes of a
model. A wire case names the catalogue facts it rests on, and the replay
fails when the catalogue of the crate says otherwise.

- `python_config_forms.json` holds the JSON forms of Python's `OpenAIConfig`:
  what it dumps of a table of keyword arguments, the inputs it refuses, the
  inputs it converts, which `OpenAiConfig` refuses, and whether each of a
  table of base URLs is OpenAI's own endpoint.
- `python_chat_cases.json` holds the wire cases: for each, a configuration,
  a request, the HTTP requests Python's `OpenAIAIProvider` sent to a fake of
  the endpoint with what the fake answered, and what Python returned, of a
  completion asked whole or streamed, or of the model listing. The
  cases are listed in `scripts/openai_chat_cases.py`, each with the kinds of
  behaviour it shows; the `python_chat_cases` test requires every kind, so a
  case is added there with its kind, and a kind is never dropped. Nothing
  that changes from a run to the next is recorded. The script:
  - removes every `OPENAI_*` variable of the environment, which the SDK
    reads its key and its base URL from when a client is given none;
  - stops the SDK's clock, at the instant the fixture names;
  - counts the ids Python mints instead of drawing them;
  - takes none of the SDK's waits, and records each;
  - leaves out the headers of the SDK and of the platform.

  It must write the same bytes twice. The fixture names the commit of
  RoomKit it ran against and whether its files were clean, when RoomKit was
  imported from its own checkout, and, on a case that calls OpenAI's own
  endpoint, what the provider read of the model's catalogue entry. It is
  written with an indent of one: one member per line, for a diff to read.
- A case says how a replay takes it. Where the openai SDK decides a
  behaviour, not RoomKit, the case says so under `sdk`: the Rust provider
  has no SDK under it. A case that fails says under `compare` what a replay
  reproduces of the failure, and whose words its message holds: RoomKit's
  and the server's are compared, the SDK's and Python's are not. The test
  derives that label again from the failure and from what the fake sent. A
  case that pins a behaviour of Python that is not settled says so under
  `questioned`: a possible defect reported upstream (`POSSIBLE_DEFECTS` of
  the cases' module), or a point where the Rust provider is planned
  otherwise and the maintainer decides (`TO_DECIDE`). The test lists both:
  a case joins or leaves the module's table and the test's with the
  decision.
- A stream's body is a list of reads, each any bytes: a read that is UTF-8
  is written as its text, any other in hexadecimal.
- The provider replays every case (`src/replay`), its fake serving a stream
  in the reads the case records, which the replay checks. A case the
  provider answers otherwise than Python is never skipped: it is named
  there, in the list its case belongs to (questioned in the fixture, a
  failure no provider reproduces, or a rule of the provider's own), with
  the reason, the differences from Python its replay shows, and what the
  provider does instead. Both are asserted: a named case that differs
  otherwise than its entry states fails as one that does otherwise.

## The Python package

`python/roomkit_providers` wraps the native module `roomkit_providers._native`
(`bindings/python`), which holds no behaviour of its own: what a provider
sends and reads stays in the provider crates.

- **What crosses is JSON text**, both ways, in the contract's forms: a
  request as one text, a whole answer as one text, a stream as the list of
  every item already received at each crossing. A stream is read on the
  bridge between two crossings, at most 64 items ahead of what Python took,
  and a crossing never waits to gather more. A failure crosses as the
  attributes of the package's exception. A key never crosses inside a JSON
  text.
- **Every call that waits returns an awaitable.** It is stopped by dropping
  its future: when its task is cancelled, when its stream is closed, and
  when the interpreter exits.
- **The tests** (`tests/python`, `unittest`, no dependency) run against the
  installed package and a local fake of the endpoint
  (`tests/python/fake_endpoint.py`). `scripts/check-python.py` builds a
  wheel and a source distribution, installs each alone in a new
  environment, checks what each contains, and runs the tests and
  `examples/local_server.py` on both. A test waits on what the fake
  observed, never on a timer.
- **The replay.** `tests/python/test_replay.py` replays the recorded wire
  cases through the package, and compares as the Rust replay does. The cases
  the provider answers otherwise than Python are read from
  `crates/roomkit-providers-openai/tests/fixtures/named_cases.json`, which
  the Rust replay writes from its own list: after a change to
  `src/replay/named.rs`, run `make named-cases` and commit the file; a test
  of the crate fails while the two differ. The replay drives a provider
  through a seam of the OpenAI crate, its `replay` feature, which sends to a
  loopback address only, sets the clock a `retry-after` date is read against
  and keeps the waits between two tries instead of taking them. The seam is
  a private method of the native module, for this replay, never for a host.
  The cases whose fake stops answering under way stay with the Rust replay,
  which moves its own clock.
- **The public API snapshot.** `tests/python/public_api_names.json` and
  `public_api_signatures.json` hold every public name, each signature, and
  the keys of each typed dict. A test fails while they differ from the
  code, a compatible addition included: the commit that changes the Python
  surface rewrites them with `make api-snapshot`.
- **Licenses.** The wheel ships the notices of every crate its extension
  links (`python/roomkit_providers/licenses/native_dependencies.txt`).
  After a change of dependencies, regenerate them with
  `python scripts/native-notices.py write`; the check fails for a crate
  with no notice. A crate published without its license file gets a copy
  of its upstream's under `bindings/python/licenses/upstream`.
- **What a distribution contains** is the repository's files and what the
  build generates: a file git ignores must not reach a wheel or a source
  distribution (`scripts/artifact_files.py`).
- **The RoomKit adapter** (`python/roomkit_providers/roomkit`) is the only
  part of the package that imports RoomKit, and it converts and nothing
  else: a context to the contract's JSON, an answer to RoomKit's models, a
  failure to RoomKit's exception. Its tests (`tests/python/roomkit`) are
  skipped, saying why, where RoomKit is not installed: the replay of the
  recorded cases through the adapter, an agent (a kit and an AI channel over
  the adapter) that answers, calls a tool and streams, and the first-token
  metric. `make python-roomkit-check` builds a wheel, installs it with its
  `roomkit` extra beside the RoomKit of `PYTHON`'s environment
  (`scripts/roomkit_env.py` names it: where it was installed from, else its
  version) and runs those tests and `examples/roomkit_agent.py` there.
  `make python-check` calls it when `PYTHON`'s environment has RoomKit, and
  says that it did not when it has none. The adapter has its own API
  snapshot, in `tests/python/roomkit`, which `make api-snapshot` rewrites
  when RoomKit is installed. The extra's range is the RoomKit the adapter
  was tested against: widen it with a run of `make python-roomkit-check` on
  the version added.
- `benchmarks/crossing.py` measures a turn through the package, through the
  adapter and through RoomKit's own provider. It is no test and no gate.

CI runs `make python-check` on CPython 3.12 and 3.14, and the binding's
MSRV check; the `Python distributions` workflow builds the Linux x86_64
wheel and the source distribution on demand and checks both. Nothing is
published from either. Neither installs RoomKit: the adapter's check runs
where a developer has it.

## Implementation boundaries

- Keep `roomkit-providers-llm` free of I/O, async runtimes, Python and
  RoomKit dependencies. `make check-contract-deps` holds its allow list; update
  the list in the commit that changes the crate's dependencies.
- Put dependency versions in `workspace.dependencies`; consuming crates enable
  only the features they need.
- The host sets endpoints, credentials, timeouts and limits explicitly. A
  provider reads no environment variable and no global state for them.
- A credential appears in no log, no debug representation and no result,
  and an error's text carries none of 8 characters or more that a server
  sent back, as it is or as the code that builds the error writes it again
  (as JSON, as a quoted value); a provider's README states what it cannot
  recognise.
- Document public items; return typed failures instead of panicking on what a
  caller or a service sends.
- Prefer `#[expect(lint, reason = "...")]` for a justified lint exception.

## Documentation

Code, comments, examples and public documentation use English and generic
data. Keep local files out of version control with your checkout's
`.git/info/exclude` and do not force-add ignored material.

## Commits

Use `type(scope): summary`, at most 72 characters, for example
`feat(llm): add the request contract`. The hook and CI share the same
checker. Record user-visible changes under `Unreleased` in
[CHANGELOG.md](CHANGELOG.md).
