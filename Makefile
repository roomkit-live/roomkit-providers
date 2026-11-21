CARGO ?= cargo
MSRV := $(shell sed -n 's/^rust-version = "\(.*\)"$$/\1/p' Cargo.toml).0
DENY_FLAGS ?=
# The interpreter of the generated files: one with RoomKit's Python package
# installed for python-forms, python-public-urls and the python-openai
# targets, the Python version RoomKit is tested with for printable-table.
# For python-check, python-wheel and api-snapshot, the interpreter the
# package is built for and tested on: CPython 3.12 or newer.
PYTHON ?= python3
PYTHON_MANIFEST := bindings/python/Cargo.toml
# The Python tools, each run at a version within its range: name a local
# installation in place of one (MATURIN=.venv/bin/maturin).
MATURIN ?= uv tool run --from 'maturin>=1.15,<2' maturin
RUFF ?= uv tool run --from 'ruff>=0.14,<1' ruff
PYTHON_SOURCES := python tests/python examples benchmarks scripts/check-python.py \
	scripts/native-notices.py scripts/artifact_files.py scripts/api-snapshot.py \
	scripts/roomkit_env.py
# Whether PYTHON's environment has RoomKit, which the adapter is tested
# against.
HAS_ROOMKIT = $(PYTHON) scripts/roomkit_env.py

.PHONY: all fmt fmt-check clippy test examples doc deny check-contract-deps msrv msrv-toolchain hooks \
	python-forms python-public-urls python-openai-config python-openai-chat-cases \
	python-openai-catalogue \
	printable-table named-cases \
	python-wheel python-check python-roomkit-check python-msrv api-snapshot

all: fmt-check clippy test examples doc deny check-contract-deps

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --workspace --all-targets --all-features --locked -- -D warnings

test:
	$(CARGO) test --workspace --all-features --locked

# Runs each example, which checks what it shows and fails otherwise. An
# example talks to a local server it starts itself: nothing leaves the
# machine.
#
# It runs with every proxy variable set to a local port nothing listens on.
# A provider reads no proxy from its environment: one that did would send
# its requests there, and the example would fail. The check is made here
# because the crates forbid unsafe code, so no test can set a variable of
# its own process. The example is built first, without the variables, so
# that cargo itself is given no proxy.
NO_SUCH_PROXY := http://127.0.0.1:9
examples:
	$(CARGO) build --locked -p roomkit-providers-openai --example local_server
	HTTP_PROXY=$(NO_SUCH_PROXY) HTTPS_PROXY=$(NO_SUCH_PROXY) ALL_PROXY=$(NO_SUCH_PROXY) \
	http_proxy=$(NO_SUCH_PROXY) https_proxy=$(NO_SUCH_PROXY) all_proxy=$(NO_SUCH_PROXY) \
	NO_PROXY= no_proxy= \
		$(CARGO) run --locked --offline -p roomkit-providers-openai --example local_server

doc:
	RUSTDOCFLAGS="-D warnings" $(CARGO) doc --workspace --no-deps --all-features --locked

deny:
	@command -v cargo-deny >/dev/null 2>&1 || { \
		echo "Install cargo-deny: cargo install cargo-deny --locked" >&2; exit 1; \
	}
	$(CARGO) deny $(DENY_FLAGS) check

check-contract-deps:
	CARGO="$(CARGO)" bash scripts/check-contract-deps.sh

msrv:
	$(CARGO) +$(MSRV) check --workspace --all-targets --all-features --locked

msrv-toolchain:
	rustup toolchain install $(MSRV) --profile minimal

hooks:
	git config core.hooksPath .githooks

# The Python package (CONTRIBUTING.md): an optional bridge over the crates.
# The Rust gates above stay free of any interpreter; these targets need
# CPython 3.12 or newer and uv.

# Builds the wheel of the Python package into dist/, optimized.
python-wheel:
	$(MATURIN) build --locked --release --interpreter $(PYTHON) --out dist

# The binding's own lint, documentation and dependency policy, the Python
# lint, then scripts/check-python.py: a wheel and a source distribution are
# built, each installed alone in a new environment, and the Python tests
# and the example run on each. The tests answer from a local fake: nothing
# is asked of a network. Where PYTHON's environment has RoomKit, the
# adapter's check follows.
python-check:
	$(CARGO) fmt --manifest-path $(PYTHON_MANIFEST) --all -- --check
	$(CARGO) clippy --manifest-path $(PYTHON_MANIFEST) --all-targets --locked -- -D warnings
	RUSTDOCFLAGS="-D warnings" $(CARGO) doc --manifest-path $(PYTHON_MANIFEST) --no-deps --locked
	$(CARGO) deny --manifest-path $(PYTHON_MANIFEST) --config deny.toml $(DENY_FLAGS) check
	$(RUFF) check $(PYTHON_SOURCES)
	$(RUFF) format --check $(PYTHON_SOURCES)
	MATURIN="$(MATURIN)" $(PYTHON) scripts/check-python.py
	@if $(HAS_ROOMKIT); then \
		$(MAKE) --no-print-directory python-roomkit-check; \
	else \
		echo "RoomKit is not installed for $(PYTHON): its adapter's check did not run (make python-roomkit-check)"; \
	fi

# The RoomKit adapter: a wheel is built and installed, in a new environment,
# with its `roomkit` extra and the RoomKit PYTHON's environment has; the
# adapter's tests (tests/python/roomkit) and its example run there. It
# fails where PYTHON's environment has no RoomKit.
python-roomkit-check:
	MATURIN="$(MATURIN)" $(PYTHON) scripts/check-python.py --roomkit

python-msrv:
	$(CARGO) +$(MSRV) check --manifest-path $(PYTHON_MANIFEST) --all-targets --locked

# Rewrites the snapshots of the Python public API from a freshly built wheel:
# the package's, and its RoomKit adapter's where PYTHON's environment has
# RoomKit. The Python tests fail while a snapshot differs from the code: the
# commit that changes the public surface rewrites it.
api-snapshot:
	MATURIN="$(MATURIN)" $(PYTHON) scripts/api-snapshot.py

# Rewrites the fixtures of what RoomKit's Python gives: the JSON forms of its
# models and the decisions of its call rules. The tests only read the
# fixtures: `make all` needs no Python.
python-forms:
	$(PYTHON) scripts/generate_python_forms.py

# Rewrites the fixture of what RoomKit's Python decides of a table of URLs,
# and the license file of the Unicode data the fixture's list of characters
# comes from. The script answers its own lookups: nothing is asked of the
# network.
python-public-urls:
	$(PYTHON) scripts/generate_python_public_urls.py

# Rewrites the fixture of the JSON forms of RoomKit's Python `OpenAIConfig`.
python-openai-config:
	$(PYTHON) scripts/generate_openai_config_forms.py

# Rewrites the catalogue of OpenAI's models the provider reads, from
# RoomKit's Python.
python-openai-catalogue:
	$(PYTHON) scripts/generate_openai_catalogue.py

# Rewrites the fixture of what RoomKit's Python OpenAI provider sends and
# reads on Chat Completions, case by case. PYTHON needs roomkit's `openai`
# extra. The script answers its own requests: nothing is sent to a network.
python-openai-chat-cases:
	$(PYTHON) scripts/generate_openai_chat_cases.py

# Rewrites the data file of the cases the OpenAI provider answers otherwise
# than Python, from the replay's own list: a replay outside the crate reads
# it, and a test of the crate fails while the two differ.
named-cases:
	NAMED_CASES_OUT=$(abspath crates/roomkit-providers-openai/tests/fixtures/named_cases.json) \
		$(CARGO) test --locked -p roomkit-providers-openai --lib the_named_cases_file

# Rewrites the table of the characters Python's `repr` writes as they are,
# from the Unicode data of PYTHON, the Python version RoomKit is tested with,
# and the license file of that data.
printable-table:
	$(PYTHON) scripts/generate_printable_ranges.py
