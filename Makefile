CARGO ?= cargo
MSRV := $(shell sed -n 's/^rust-version = "\(.*\)"$$/\1/p' Cargo.toml).0
DENY_FLAGS ?=
# The interpreter of the generated files: one with RoomKit's Python package
# installed for python-public-urls, python-proxy-cases and the python-openai
# and python-anthropic targets.
# For python-check, python-wheel and api-snapshot, the interpreter the
# package is built for and tested on: CPython 3.12 or newer.
PYTHON ?= python3
PYTHON_MANIFEST := bindings/python/Cargo.toml
# Runs a command with no network but loopback, and nothing where this machine
# lets a user make no such place (scripts/no-network.sh).
NO_NETWORK := scripts/no-network.sh
# The Python tools, each run at a version within its range: name a local
# installation in place of one (MATURIN=.venv/bin/maturin).
MATURIN ?= uv tool run --from 'maturin>=1.15,<2' maturin
RUFF ?= uv tool run --from 'ruff>=0.14,<1' ruff
PYTHON_SOURCES := python tests/python examples benchmarks scripts/check-python.py \
	scripts/native-notices.py scripts/artifact_files.py scripts/api-snapshot.py \
	scripts/roomkit_env.py scripts/wheel_paths.py

.PHONY: all fmt fmt-check clippy no-network-test test examples doc deny \
	msrv msrv-toolchain hooks redis-test \
	python-public-urls python-proxy-cases python-openai-config \
	python-openai-chat-cases \
	python-openai-catalogue \
	python-anthropic-config python-anthropic-messages-cases python-anthropic-catalogue \
	named-cases \
	python-wheel python-check python-msrv api-snapshot

all: fmt-check clippy no-network-test test examples doc deny

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --workspace --all-targets --all-features --locked -- -D warnings

# Checks the script the tests, the examples and the benches go through
# (scripts/no-network-test.sh): that its command has no network whatever the
# environment names, and that it runs nothing where it can make no
# namespace. The check says where it was run, and what it could not show
# there.
no-network-test:
	bash scripts/no-network-test.sh

# The tests are built, then run where no network is reachable but loopback
# (scripts/no-network.sh): each answers from a local fake, and one that lost
# its way to it fails there instead of reaching a service.
#
# No Redis server is reachable there either: the tests of the turn backend
# on Redis that need one skip themselves, whatever REDIS_URL names
# (`make redis-test` runs them).
test:
	$(CARGO) test --workspace --all-features --locked --no-run
	$(NO_NETWORK) env -u REDIS_URL $(CARGO) test --workspace --all-features --locked --offline

# The turn backend on Redis (roomkit-providers-redis) on a Redis server:
# its functional tests, the interop with RoomKit's Python where
# ROOMKIT_PYTHON names the interpreter of a RoomKit checkout, and its
# example of two processes. REDIS_URL names the server, a local one for
# tests: this target alone runs outside scripts/no-network.sh, because a
# server of the machine is out of reach from a namespace with no network.
#
#     docker run -d --rm -p 127.0.0.1:6379:6379 redis:7
#     REDIS_URL=redis://127.0.0.1:6379 make redis-test
redis-test:
	@test -n "$$REDIS_URL" || { echo "redis-test: REDIS_URL is not set" >&2; exit 1; }
	$(CARGO) test --locked --no-fail-fast -p roomkit-providers-redis
	$(CARGO) run --locked -p roomkit-providers-redis --example two_processes

# Runs each example, which checks what it shows and fails otherwise. An
# example talks to local servers it starts itself: nothing leaves the
# machine.
#
# behind_a_proxy is the proof that a provider reads its proxies and its
# certificate authorities from the environment of its process: it runs
# itself again as a child whose environment names a local proxy and the
# authority of a local TLS server, and checks that the request for an https
# endpoint went through a tunnel of the proxy, that the request for an http
# endpoint was handed to the proxy, and that the host NO_PROXY names was
# reached directly. The crates forbid unsafe code, so no test sets a
# variable of its own process: a child is given its environment instead.
# The examples of RoomKit agents on these providers (rust-examples): every
# file of the directory, so an example missing from its manifest fails the
# gate. Each runs against a local fake of its provider's wire and checks
# what it shows.
AGENT_EXAMPLES := $(basename $(notdir $(wildcard rust-examples/*.rs)))

# The examples are built first, outside the namespace that runs them.
examples:
	$(CARGO) build --locked -p roomkit-providers-openai -p roomkit-providers-anthropic \
		-p roomkit-providers-examples --examples
	$(NO_NETWORK) $(CARGO) run --locked --offline -p roomkit-providers-openai --example local_server
	$(NO_NETWORK) $(CARGO) run --locked --offline -p roomkit-providers-openai --example behind_a_proxy
	$(NO_NETWORK) $(CARGO) run --locked --offline -p roomkit-providers-anthropic --example local_messages
	@for example in $(AGENT_EXAMPLES); do \
		echo "== $$example"; \
		$(NO_NETWORK) $(CARGO) run --quiet --locked --offline \
			-p roomkit-providers-examples --example "$$example" || exit 1; \
	done

doc:
	RUSTDOCFLAGS="-D warnings" $(CARGO) doc --workspace --no-deps --all-features --locked

deny:
	@command -v cargo-deny >/dev/null 2>&1 || { \
		echo "Install cargo-deny: cargo install cargo-deny --locked" >&2; exit 1; \
	}
	$(CARGO) deny $(DENY_FLAGS) check

msrv:
	$(CARGO) +$(MSRV) check --workspace --all-targets --all-features --locked

msrv-toolchain:
	rustup toolchain install $(MSRV) --profile minimal

hooks:
	git config core.hooksPath .githooks

# The Python package (CONTRIBUTING.md): an optional bridge over the crates.
# The Rust gates above stay free of any interpreter; these targets need
# CPython 3.12 or newer and uv.

# Builds the wheel of the Python package into dist/, optimized. The paths of
# the machine that builds it stay out of it: the compiler writes the home
# directory, the registry of crates and the checkout as the names below (a
# later mapping wins, so the checkout's does under the home directory), and
# the bill of materials the build adds is rewritten the same way.
# The C a crate compiles is told the same as the Rust.
NAMED := $(HOME)=/home $(HOME)/.cargo=/cargo $(CURDIR)=/roomkit-providers
python-wheel:
	RUSTFLAGS="$(RUSTFLAGS) $(addprefix --remap-path-prefix=,$(NAMED))" \
	CFLAGS="$(CFLAGS) $(addprefix -ffile-prefix-map=,$(NAMED))" \
		$(MATURIN) build --locked --release --interpreter $(PYTHON) --out dist
	$(PYTHON) scripts/wheel_paths.py dist/roomkit_providers-*.whl

# The binding's own lint, documentation and dependency policy, the Python
# lint, then scripts/check-python.py: a wheel and a source distribution are
# built, each installed in a new environment beside the RoomKit PYTHON's
# environment has, and the Python tests and the examples run on each. The
# tests answer from a local fake, and they and the examples run where no
# network is reachable but loopback (scripts/no-network.sh).
python-check:
	$(CARGO) fmt --manifest-path $(PYTHON_MANIFEST) --all -- --check
	$(CARGO) clippy --manifest-path $(PYTHON_MANIFEST) --all-targets --all-features --locked -- -D warnings
	RUSTDOCFLAGS="-D warnings" $(CARGO) doc --manifest-path $(PYTHON_MANIFEST) --no-deps --all-features --locked
	$(CARGO) deny --manifest-path $(PYTHON_MANIFEST) --config deny.toml $(DENY_FLAGS) check
	$(RUFF) check $(PYTHON_SOURCES)
	$(RUFF) format --check $(PYTHON_SOURCES)
	MATURIN="$(MATURIN)" $(PYTHON) scripts/check-python.py

python-msrv:
	$(CARGO) +$(MSRV) check --manifest-path $(PYTHON_MANIFEST) --all-targets --all-features --locked

# Rewrites the snapshot of the Python public API from a freshly built wheel.
# The Python tests fail while the snapshot differs from the code: the commit
# that changes the public surface rewrites it.
api-snapshot:
	MATURIN="$(MATURIN)" $(PYTHON) scripts/api-snapshot.py

# Rewrites the fixture of what RoomKit's Python decides of a table of URLs,
# and the license file of the Unicode data the fixture's list of characters
# comes from. The script answers its own lookups: nothing is asked of the
# network.
python-public-urls:
	$(PYTHON) scripts/generate_python_public_urls.py

# Rewrites the fixture of which proxy the client of RoomKit's Python providers
# takes for a table of environments. PYTHON needs roomkit's `openai` extra.
# The script builds clients and sends nothing.
python-proxy-cases:
	$(PYTHON) scripts/generate_python_proxy_cases.py

# Rewrites the fixture of the JSON forms of RoomKit's Python `OpenAIConfig`.
python-openai-config:
	$(PYTHON) scripts/generate_openai_config_forms.py

# Rewrites the catalogues of the models the OpenAI crate's providers read,
# OpenAI's and each vendor's, from RoomKit's Python.
python-openai-catalogue:
	$(PYTHON) scripts/generate_openai_catalogue.py

# Rewrites the fixture of what RoomKit's Python OpenAI provider sends and
# reads on Chat Completions, case by case. PYTHON needs roomkit's `openai`
# extra. The script answers its own requests: nothing is sent to a network.
python-openai-chat-cases:
	$(PYTHON) scripts/generate_openai_chat_cases.py

# Rewrites the fixture of the JSON forms of RoomKit's Python `AnthropicConfig`.
python-anthropic-config:
	$(PYTHON) scripts/generate_anthropic_config_forms.py

# Rewrites the catalogue of Anthropic's models the provider reads, from
# RoomKit's Python.
python-anthropic-catalogue:
	$(PYTHON) scripts/generate_anthropic_catalogue.py

# Rewrites the fixture of what RoomKit's Python Anthropic provider sends and
# reads on the Messages API, case by case. PYTHON needs roomkit's `anthropic`
# extra. The script answers its own requests: nothing is sent to a network.
python-anthropic-messages-cases:
	$(PYTHON) scripts/generate_anthropic_messages_cases.py

# Rewrites, for each provider, the data file of the cases it answers
# otherwise than Python, from the replay's own list: a replay outside the
# crate reads it, and a test of the crate fails while the two differ.
named-cases:
	NAMED_CASES_OUT=$(abspath crates/roomkit-providers-openai/tests/fixtures/named_cases.json) \
		$(NO_NETWORK) $(CARGO) test --locked --offline -p roomkit-providers-openai --lib the_named_cases_file
	NAMED_CASES_OUT=$(abspath crates/roomkit-providers-anthropic/tests/fixtures/named_cases.json) \
		$(NO_NETWORK) $(CARGO) test --locked --offline -p roomkit-providers-anthropic --lib the_named_cases_file

