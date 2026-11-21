CARGO ?= cargo
MSRV := $(shell sed -n 's/^rust-version = "\(.*\)"$$/\1/p' Cargo.toml).0
DENY_FLAGS ?=
# The interpreter of the generated files: one with RoomKit's Python package
# installed for python-forms and python-public-urls, the Python version
# RoomKit is tested with for printable-table.
PYTHON ?= python3

.PHONY: all fmt fmt-check clippy test doc deny check-core-deps msrv msrv-toolchain hooks \
	python-forms python-public-urls printable-table

all: fmt-check clippy test doc deny check-core-deps

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --workspace --all-targets --all-features --locked -- -D warnings

test:
	$(CARGO) test --workspace --all-features --locked

doc:
	RUSTDOCFLAGS="-D warnings" $(CARGO) doc --workspace --no-deps --all-features --locked

deny:
	@command -v cargo-deny >/dev/null 2>&1 || { \
		echo "Install cargo-deny: cargo install cargo-deny --locked" >&2; exit 1; \
	}
	$(CARGO) deny $(DENY_FLAGS) check

check-core-deps:
	CARGO="$(CARGO)" bash scripts/check-core-deps.sh

msrv:
	$(CARGO) +$(MSRV) check --workspace --all-targets --all-features --locked

msrv-toolchain:
	rustup toolchain install $(MSRV) --profile minimal

hooks:
	git config core.hooksPath .githooks

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

# Rewrites the table of the characters Python's `repr` writes as they are,
# from the Unicode data of PYTHON, the Python version RoomKit is tested with,
# and the license file of that data.
printable-table:
	$(PYTHON) scripts/generate_printable_ranges.py
