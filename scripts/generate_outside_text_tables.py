#!/usr/bin/env python3
"""Write the Unicode tables the contract reads text from outside with.

The contract's ``fence`` and ``identifier`` (``src/outside``) decide as
RoomKit's Python does, ``roomkit._text.fence`` and
``roomkit._text.identifier``. What Python decides of one character is written
here as Rust tables, so that neither function depends on the Unicode version
of Rust's standard library:

- ``look_alikes.rs``: what each character reads as in a tag, a bracket, a
  slash, a letter or a run of letters, as the classes of
  ``roomkit._lookalike`` match it (from Unicode's confusables);
- ``classes.rs``: the characters RoomKit's patterns take for the room around a
  tag's name, for its separator and for what its letters are read past, the
  combining marks a tag is read without, the characters an identifier keeps
  and those ``str.isalnum()`` accepts;
- ``normalization.rs``: canonical composition (NFC) as ``unicodedata`` gives
  it, each character's full canonical decomposition, the canonical combining
  classes and the primary composites.

The tables come from the interpreter that runs this script and the ``roomkit``
it has installed, which each file's header records: run it with the Python
version RoomKit is tested with, so that both read one Unicode version.

The tables are derived from Unicode data, under the Unicode License V3. The
script writes that license's notice in each table's header and as the license
file the crate ships beside its own, ``LICENSE-UNICODE``.
"""

from __future__ import annotations

import re
import sys
import unicodedata
from collections.abc import Callable, Iterator
from pathlib import Path

import roomkit
from roomkit import _lookalike, _lookalike_data, _text
from unicode_notice import UNICODE_NOTICE

REPOSITORY = Path(__file__).resolve().parent.parent
CRATE = REPOSITORY / "crates/roomkit-providers-llm"
TABLES = CRATE / "src/outside/tables"
LICENSE = CRATE / "LICENSE-UNICODE"

SURROGATES = range(0xD800, 0xE000)
HANGUL_SYLLABLES = range(0xAC00, 0xD7A4)


def scalar_values() -> Iterator[str]:
    """Every Unicode scalar value, as a character."""
    return (chr(code) for code in range(0x110000) if code not in SURROGATES)


def ranges(accepts: Callable[[str], object]) -> list[str]:
    """The inclusive ranges of the characters ``accepts`` takes, as Rust pairs."""
    found: list[tuple[int, int]] = []
    start = previous = None
    for char in scalar_values():
        if not accepts(char):
            continue
        code = ord(char)
        if start is None or previous != code - 1:
            if start is not None:
                found.append((start, previous))
            start = code
        previous = code
    if start is not None:
        found.append((start, previous))
    return [f"({rust_char(first)}, {rust_char(last)})" for first, last in found]


def rust_char(code: int) -> str:
    return f"'\\u{{{code:x}}}'"


def rust_str(text: str) -> str:
    return '"' + "".join(f"\\u{{{ord(char):x}}}" for char in text) + '"'


def commented(text: str) -> list[str]:
    """``text`` as the lines of a Rust module comment."""
    return [f"//! {line}" if line else "//!" for line in text.splitlines()]


def table(name: str, kind: str, items: list[str], per_line: int, doc: str) -> str:
    """One Rust table, documented, its items ``per_line`` to a line."""
    lines = [f"/// {line}" for line in doc.splitlines()]
    lines.append("#[rustfmt::skip]")
    lines.append(f"pub(in crate::outside) static {name}: &[{kind}] = &[")
    lines += [
        "    " + " ".join(f"{item}," for item in items[at : at + per_line])
        for at in range(0, len(items), per_line)
    ]
    lines.append("];")
    return "\n".join(lines)


def matches(pattern: str) -> Callable[[str], object]:
    """Whether a character is one ``pattern`` matches, read without case as
    RoomKit compiles its patterns."""
    return re.compile(pattern, re.IGNORECASE).fullmatch


def reads() -> list[str]:
    """Each character a tag reads as another, with every character and run of
    letters whose class takes it: the classes ``fence`` builds a tag's
    pattern from, ``char_class`` of a letter, a digit or a sign and
    ``_sequences`` of a run of letters."""
    classes = [
        (target, matches(_lookalike.char_class(target)))
        for target in sorted(_lookalike.lookalikes())
    ]
    classes += [
        (run, matches(pattern))
        for run, pattern in sorted(_lookalike._sequences(phrase=False).items())
    ]
    found = []
    for char in scalar_values():
        runs = [run for run, taken in classes if taken(char)]
        if runs:
            found.append(f"({rust_char(ord(char))}, {rust_str(' '.join(runs))})")
    return found


def look_alikes() -> str:
    return table(
        "READS",
        "(char, &str)",
        reads(),
        3,
        "What each character reads as in a tag, sorted by character: the letters,\n"
        "digits and signs (`<`, `>`, `/`, `_`, `:`, `'`, `\"`) whose class in\n"
        "`roomkit._lookalike.char_class` takes it, read without case, and the\n"
        "runs of letters whose class in `_sequences` does (`vi` for `ⅵ`),\n"
        "separated by spaces.",
    )


def classes() -> str:
    tables = (
        (
            "ROOM",
            matches(_lookalike.SPACE),
            "The room between a tag's brackets, slash and name: `roomkit._lookalike.SPACE`.",
        ),
        (
            "SEPARATORS",
            matches(_lookalike.SEPARATOR),
            "What reads as a tag name's separator, or is read past where one sits:\n"
            "`roomkit._lookalike.SEPARATOR`.",
        ),
        (
            "JOINT",
            matches(_lookalike.JOINT),
            "The characters a name's letters are read past: `roomkit._lookalike.JOINT`.",
        ),
        (
            "HIDDEN_MARKS",
            lambda char: not _lookalike.without_hidden_marks(char),
            "The combining marks a tag is read without, once read as written: those\n"
            "`roomkit._lookalike.without_hidden_marks` drops.",
        ),
        (
            "NAME_CHARS",
            _text._a_name_s,
            "The letters, marks and digits an identifier keeps: `roomkit._text._a_name_s`.",
        ),
        (
            "ALNUM",
            str.isalnum,
            "The characters Python's `str.isalnum()` accepts.",
        ),
    )
    return "\n\n".join(
        table(name, "(char, char)", ranges(accepts), 4, doc + "\nInclusive ranges, sorted.")
        for name, accepts, doc in tables
    )


def primary_composite(char: str) -> tuple[int, int, int] | None:
    """The pair ``char`` decomposes into, with ``char``, when NFC composes the
    pair back."""
    mapping = unicodedata.decomposition(char)
    if not mapping or mapping.startswith("<"):
        return None
    pair = [int(code, 16) for code in mapping.split()]
    if len(pair) != 2 or unicodedata.normalize("NFC", chr(pair[0]) + chr(pair[1])) != char:
        return None
    return pair[0], pair[1], ord(char)


def normalization() -> str:
    decompositions, combining, composites = [], [], []
    for char in scalar_values():
        code = ord(char)
        if code not in HANGUL_SYLLABLES:
            decomposed = unicodedata.normalize("NFD", char)
            if decomposed != char:
                decompositions.append(f"({rust_char(code)}, {rust_str(decomposed)})")
            composite = primary_composite(char)
            if composite:
                composites.append(composite)
        if unicodedata.combining(char):
            combining.append(f"({rust_char(code)}, {unicodedata.combining(char)})")
    return "\n\n".join(
        (
            table(
                "DECOMPOSITIONS",
                "(char, &str)",
                decompositions,
                3,
                "Each character's full canonical decomposition, sorted by character;\n"
                "the Hangul syllables are decomposed by their algorithm.",
            ),
            table(
                "COMBINING_CLASSES",
                "(char, u8)",
                combining,
                6,
                "The canonical combining class of each character that has one, sorted.",
            ),
            table(
                "COMPOSITIONS",
                "(char, char, char)",
                [
                    f"({rust_char(first)}, {rust_char(second)}, {rust_char(composite)})"
                    for first, second, composite in sorted(composites)
                ],
                3,
                "The primary composites: a starter, the character that follows it, and\n"
                "what they compose into, sorted by the pair; Hangul by its algorithm.",
            ),
        )
    )


def written_from() -> str:
    """What every table's header says of where it comes from."""
    python = sys.version.split()[0]
    return (
        "Generated by `scripts/generate_outside_text_tables.py` from roomkit\n"
        f"{roomkit.__version__} and the Unicode {unicodedata.unidata_version} data of "
        f"Python {python}. Do not edit; run\n"
        "the script again.\n"
        "\n"
        "The data is derived from Unicode data files, under this notice:\n"
        "\n"
    )


def write(name: str, what: str, tables: str) -> None:
    path = TABLES / name
    lines = commented(what + "\n" + written_from() + UNICODE_NOTICE)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n\n" + tables + "\n", encoding="utf-8")
    print(f"Wrote {path.relative_to(REPOSITORY)}")


def main() -> None:
    write(
        "look_alikes.rs",
        "What each character reads as in a tag (RFC §6.4): the look-alike classes\n"
        "of `RoomKit`'s Python implementation, from Unicode's confusables.txt\n"
        f"{_lookalike_data.CONFUSABLES_VERSION}.\n",
        look_alikes(),
    )
    write(
        "classes.rs",
        "The classes of characters a tag and an identifier are read with\n"
        "(RFC §6.4), as `RoomKit`'s Python implementation decides them.\n",
        classes(),
    )
    write(
        "normalization.rs",
        "Canonical composition (NFC) as Python's `unicodedata` gives it, which an\n"
        "identifier is composed with.\n",
        normalization(),
    )
    LICENSE.write_text(UNICODE_NOTICE, encoding="utf-8")
    print(
        f"Wrote {LICENSE.relative_to(REPOSITORY)}: roomkit {roomkit.__version__}, "
        f"Unicode {unicodedata.unidata_version}, Python {sys.version.split()[0]}"
    )


if __name__ == "__main__":
    main()
