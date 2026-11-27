CARGO ?= cargo
MSRV := $(shell sed -n 's/^rust-version = "\(.*\)"$$/\1/p' Cargo.toml).0
# The crates moved from RoomKit's Rust core keep the Rust release it
# supports, below the workspace's: its stores and its telemetry provider.
ENGINE_MSRV := $(shell sed -n 's/^rust-version = "\(.*\)"$$/\1/p' crates/roomkit-providers-sqlite/Cargo.toml).0
ENGINE_MSRV_CRATES := -p roomkit-providers-sqlite -p roomkit-providers-postgres -p roomkit-providers-opentelemetry
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
	scripts/roomkit_env.py scripts/wheel_paths.py scripts/check-postgres-consumer.py

.PHONY: all fmt fmt-check clippy no-network-test test examples doc deny \
	msrv msrv-toolchain hooks redis-test postgres-test postgres-fixture sqlite-fixture \
	python-public-urls python-proxy-cases python-openai-config \
	python-openai-chat-cases \
	python-openai-catalogue \
	python-anthropic-config python-anthropic-messages-cases python-anthropic-catalogue \
	python-gemini-config python-gemini-cases python-gemini-catalogue \
	python-mistral-config python-mistral-cases python-mistral-catalogue \
	python-ollama-config python-ollama-cases python-ollama-catalogue python-ollama-images \
	python-polargrid-config python-polargrid-cases python-polargrid-catalogue \
	python-vertex-config python-vertex-cases \
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
# No Redis or Postgres server is reachable there either: the tests of the
# turn backend on Redis and of the Postgres store that need one skip
# themselves, whatever REDIS_URL and POSTGRES_DSN name (`make redis-test`
# and `make postgres-test` run them).
test:
	$(CARGO) test --workspace --all-features --locked --no-run
	$(NO_NETWORK) env -u REDIS_URL -u POSTGRES_DSN $(CARGO) test --workspace --all-features --locked --offline

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

# The Postgres store (roomkit-providers-postgres) on a Postgres server: the
# check that an application's own workspace resolves the patched driver the
# crate's README names, its tests, the cross-read with RoomKit's Python where
# ROOMKIT_PYTHON names the interpreter of a RoomKit checkout, and its example.
# POSTGRES_DSN names the server, a local one for tests: like redis-test, this
# target runs outside scripts/no-network.sh.
#
#     docker run -d --rm -e POSTGRES_PASSWORD=local -p 127.0.0.1:5432:5432 postgres:16
#     POSTGRES_DSN=postgresql://postgres:local@127.0.0.1:5432/postgres make postgres-test
postgres-test:
	@test -n "$$POSTGRES_DSN" || { echo "postgres-test: POSTGRES_DSN is not set" >&2; exit 1; }
	python3 scripts/check-postgres-consumer.py
	$(CARGO) test --locked --no-fail-fast -p roomkit-providers-postgres -- --test-threads=4
	$(CARGO) run --locked -p roomkit-providers-postgres --example room_on_postgres

# The SQLite store's cross-read fixture: a SQLite file the Python
# implementation's SQLiteStore writes, which
# crates/roomkit-providers-sqlite/tests/cross_read.rs reads and rewrites with
# nothing lost. It is written by tests/fixtures/make_fixture.py with the
# interpreter of a roomkit checkout (ROOMKIT_DIR, beside this repository's
# main one), whose commit, on roomkit's origin/main, is recorded in
# tests/fixtures/ROOMKIT_COMMIT, and the file's fingerprint in
# tests/fixtures/FIXTURE_SHA256, which the test checks. The file is never
# edited here: change the generator, and write it again.
ROOMKIT_DIR ?= $(abspath $(dir $(shell git rev-parse --path-format=absolute --git-common-dir))../roomkit)
export ROOMKIT_DIR
SQLITE_FIXTURES := crates/roomkit-providers-sqlite/tests/fixtures

sqlite-fixture:
	@commit=$$(git -C "$$ROOMKIT_DIR" rev-parse --verify HEAD) && \
	if ! git -C "$$ROOMKIT_DIR" diff --quiet HEAD -- src; then \
		echo "roomkit at $$ROOMKIT_DIR has changes under src/: write the fixture from a commit"; exit 1; \
	fi && \
	if ! git -C "$$ROOMKIT_DIR" merge-base --is-ancestor "$$commit" origin/main; then \
		echo "roomkit $$commit is not on origin/main: push it and fetch first"; exit 1; \
	fi && \
	PYTHONDONTWRITEBYTECODE=1 "$$ROOMKIT_DIR/.venv/bin/python" -I \
		$(SQLITE_FIXTURES)/make_fixture.py $(SQLITE_FIXTURES)/python-v14.db && \
	echo "$$commit" > $(SQLITE_FIXTURES)/ROOMKIT_COMMIT && \
	sha256sum $(SQLITE_FIXTURES)/python-v14.db | cut -c1-64 > $(SQLITE_FIXTURES)/FIXTURE_SHA256 && \
	echo "sqlite fixture written by roomkit $$commit"

# The Postgres store's cross-read fixture: the rows the Python
# implementation's PostgresStore writes, as JSON, which
# crates/roomkit-providers-postgres/tests/cross_read.rs loads, reads and
# rewrites with nothing lost; its schema.sql is Python's SCHEMA, byte for
# byte. Both are written with the interpreter of a roomkit checkout
# (ROOMKIT_DIR) at a commit on its origin/main, recorded in
# tests/fixtures/ROOMKIT_COMMIT, the rows against the server POSTGRES_DSN
# names.
POSTGRES_CRATE := crates/roomkit-providers-postgres

postgres-fixture:
	@commit=$$(git -C "$$ROOMKIT_DIR" rev-parse --verify HEAD) && \
	if ! git -C "$$ROOMKIT_DIR" diff --quiet HEAD -- src; then \
		echo "roomkit at $$ROOMKIT_DIR has changes under src/: write the fixture from a commit"; exit 1; \
	fi && \
	if ! git -C "$$ROOMKIT_DIR" merge-base --is-ancestor "$$commit" origin/main; then \
		echo "roomkit $$commit is not on origin/main: push it and fetch first"; exit 1; \
	fi && \
	PYTHONDONTWRITEBYTECODE=1 "$$ROOMKIT_DIR/.venv/bin/python" -c \
		'from roomkit.store.postgres_schema import SCHEMA; import sys; sys.stdout.write(SCHEMA)' \
		> $(POSTGRES_CRATE)/src/schema.sql && \
	PYTHONDONTWRITEBYTECODE=1 "$$ROOMKIT_DIR/.venv/bin/python" -I \
		$(POSTGRES_CRATE)/tests/fixtures/make_fixture.py $(POSTGRES_CRATE)/tests/fixtures/python-rows.json && \
	echo "$$commit" > $(POSTGRES_CRATE)/tests/fixtures/ROOMKIT_COMMIT && \
	sha256sum $(POSTGRES_CRATE)/tests/fixtures/python-rows.json | cut -c1-64 > $(POSTGRES_CRATE)/tests/fixtures/FIXTURE_SHA256 && \
	echo "postgres fixture written by roomkit $$commit"

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
		-p roomkit-providers-llamacpp -p roomkit-providers-examples --examples
	$(NO_NETWORK) $(CARGO) run --locked --offline -p roomkit-providers-openai --example local_server
	$(NO_NETWORK) $(CARGO) run --locked --offline -p roomkit-providers-openai --example behind_a_proxy
	$(NO_NETWORK) $(CARGO) run --locked --offline -p roomkit-providers-anthropic --example local_messages
	$(NO_NETWORK) $(CARGO) run --locked --offline -p roomkit-providers-opentelemetry --example one_trace
	$(NO_NETWORK) $(CARGO) run --locked --offline -p roomkit-providers-sqlite --example room_on_sqlite
	$(NO_NETWORK) $(CARGO) run --locked --offline -p roomkit-providers-llamacpp --example managed_gguf
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
	$(CARGO) +$(ENGINE_MSRV) check $(ENGINE_MSRV_CRATES) --all-targets --all-features --locked

msrv-toolchain:
	rustup toolchain install $(MSRV) --profile minimal
	rustup toolchain install $(ENGINE_MSRV) --profile minimal

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

# Rewrites the fixture of the JSON forms of RoomKit's Python `GeminiConfig`.
python-gemini-config:
	$(PYTHON) scripts/generate_gemini_config_forms.py

# Rewrites the catalogue of Gemini's models the provider reads, from
# RoomKit's Python.
python-gemini-catalogue:
	$(PYTHON) scripts/generate_gemini_catalogue.py

# Rewrites the fixture of what RoomKit's Python Gemini provider sends and
# reads on the Developer API, case by case. PYTHON needs roomkit's `gemini`
# extra. The script answers its own requests: nothing is sent to a network.
python-gemini-cases:
	$(PYTHON) scripts/generate_gemini_cases.py

# Rewrites the fixture of the JSON forms of RoomKit's Python `MistralConfig`.
python-mistral-config:
	$(PYTHON) scripts/generate_mistral_config_forms.py

# Rewrites the catalogue of Mistral's models the provider reads, from
# RoomKit's Python.
python-mistral-catalogue:
	$(PYTHON) scripts/generate_mistral_catalogue.py

# Rewrites the fixture of what RoomKit's Python Mistral provider sends and
# reads on Mistral's chat completions, case by case. PYTHON needs roomkit's
# `mistral` extra. The script answers its own requests: nothing is sent to a
# network.
python-mistral-cases:
	$(PYTHON) scripts/generate_mistral_cases.py

# Rewrites the fixture of the JSON forms of RoomKit's Python `OllamaConfig`.
python-ollama-config:
	$(PYTHON) scripts/generate_ollama_config_forms.py

# Rewrites the catalogue of Ollama's models the provider reads, from
# RoomKit's Python.
python-ollama-catalogue:
	$(PYTHON) scripts/generate_ollama_catalogue.py

# Rewrites the fixture of what the ollama SDK sends of an image text, or
# refuses. PYTHON needs roomkit's `ollama` extra.
python-ollama-images:
	$(PYTHON) scripts/generate_ollama_images.py

# Rewrites the fixture of what RoomKit's Python Ollama provider sends and
# reads on Ollama's native API, case by case. PYTHON needs roomkit's
# `ollama` extra. The script answers its own requests: nothing is sent to a
# network.
python-ollama-cases:
	$(PYTHON) scripts/generate_ollama_cases.py

# Rewrites the fixture of the JSON forms of RoomKit's Python
# `PolarGridConfig`.
python-polargrid-config:
	$(PYTHON) scripts/generate_polargrid_config_forms.py

# Rewrites the catalogue of PolarGrid's models and edges the provider reads,
# from RoomKit's Python and polargrid-sdk.
python-polargrid-catalogue:
	$(PYTHON) scripts/generate_polargrid_catalogue.py

# Rewrites the fixture of what RoomKit's Python PolarGrid provider sends and
# reads on an edge's chat completions and on the autorouter, case by case.
# PYTHON needs roomkit's `polargrid` extra. The script answers its own
# requests: nothing is sent to a network.
python-polargrid-cases:
	$(PYTHON) scripts/generate_polargrid_cases.py

# Rewrites the fixture of the JSON forms of RoomKit's Python
# `GeminiVertexConfig`, its billing labels by Google's rules among them.
python-vertex-config:
	$(PYTHON) scripts/generate_vertex_config_forms.py

# Rewrites the fixture of what RoomKit's Python Gemini provider on Vertex AI
# sends and reads: google-auth's token request for a service-account key,
# then Vertex AI's, case by case. PYTHON needs roomkit's `gemini` extra. The
# script answers its own requests: nothing is sent to a network.
python-vertex-cases:
	$(PYTHON) scripts/generate_vertex_cases.py

# Rewrites, for each provider, the data file of the cases it answers
# otherwise than Python, from the replay's own list: a replay outside the
# crate reads it, and a test of the crate fails while the two differ.
named-cases:
	NAMED_CASES_OUT=$(abspath crates/roomkit-providers-openai/tests/fixtures/named_cases.json) \
		$(NO_NETWORK) $(CARGO) test --locked --offline -p roomkit-providers-openai --lib the_named_cases_file
	NAMED_CASES_OUT=$(abspath crates/roomkit-providers-anthropic/tests/fixtures/named_cases.json) \
		$(NO_NETWORK) $(CARGO) test --locked --offline -p roomkit-providers-anthropic --lib the_named_cases_file
	NAMED_CASES_OUT=$(abspath crates/roomkit-providers-gemini/tests/fixtures/named_cases.json) \
		$(NO_NETWORK) $(CARGO) test --locked --offline -p roomkit-providers-gemini --lib the_named_cases_file
	NAMED_CASES_OUT=$(abspath crates/roomkit-providers-mistral/tests/fixtures/named_cases.json) \
		$(NO_NETWORK) $(CARGO) test --locked --offline -p roomkit-providers-mistral --lib the_named_cases_file
	NAMED_CASES_OUT=$(abspath crates/roomkit-providers-ollama/tests/fixtures/named_cases.json) \
		$(NO_NETWORK) $(CARGO) test --locked --offline -p roomkit-providers-ollama --lib the_named_cases_file
	NAMED_CASES_OUT=$(abspath crates/roomkit-providers-polargrid/tests/fixtures/named_cases.json) \
		$(NO_NETWORK) $(CARGO) test --locked --offline -p roomkit-providers-polargrid --lib the_named_cases_file
	VERTEX_NAMED_CASES_OUT=$(abspath crates/roomkit-providers-gemini/tests/fixtures/vertex_named_cases.json) \
		$(NO_NETWORK) $(CARGO) test --locked --offline -p roomkit-providers-gemini --lib vertex::replay::the_named_cases_file

.PHONY: python-llamacpp-cases
python-llamacpp-cases:
	$(PYTHON) scripts/generate_llamacpp_cases.py
