#!/usr/bin/env python3
"""Write what the ollama SDK makes of an image RoomKit's Ollama provider hands
it, for the Ollama crate: the text it sends, or why it refuses it.

RoomKit's provider hands the SDK a data URI's payload, and any other URL as
it is (``_ollama_image_payload``). The SDK's ``Image`` reads a path that
exists from the disk, refuses a path of a picture that does not, and sends
any other text Python's ``base64.b64decode`` takes, as it is. The table holds
texts that are no path: what ``b64decode``'s lenient reading takes, and what
it refuses.

Run it with a Python interpreter that has ``roomkit`` and its ``ollama``
extra installed.
"""

from __future__ import annotations

import importlib.metadata
import json
import platform
from pathlib import Path

import ollama
import roomkit
from ollama._types import Image
from pydantic_core import PydanticSerializationError

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURE = REPOSITORY / "crates/roomkit-providers-ollama/tests/fixtures/python_images.json"

TEXTS = [
    "iVBORw0KGgo=",
    "iVBORw0KGgo",
    "iVBORw0KGg==",
    "iVBORw0KGg=",
    "QUJD",
    "QUJDRA",
    "QUJDRA=",
    "QUJDRA==",
    "QUJDRA===",
    "QUJDR",
    "Q",
    "QU",
    "QUJ",
    "",
    "====",
    "=QUJD",
    "QU=JD",
    "QUJD=QUJD",
    "QU==QUJD",
    "QUJDRA==QUJD",
    "QUJD\nREVG",
    "QU JD",
    "QUJD!",
    "!!!!",
    "QUJD-_",
    "a.b.c.d",
    "https://images.example/cat",
    "https://images.example/cat.jpg",
    "https://images.example/cat.JPEG",
    "picture.PNG",
    "picture.webp",
    "picture.gif",
    "no/such/picture.png",
    "abcde.jpg",
    "abcde.gif",
    "Q===",
    "QU==",
    "café",
    "not an image at all",
    "data:image/png;base64,iVBORw0KGgo=",
]


def made(text: str) -> dict[str, str]:
    """What the SDK makes of ``text``: what it sends, or the words of its
    refusal."""
    try:
        return {"sent": Image(value=text).model_dump()}
    except PydanticSerializationError as error:
        return {"refused": str(error).removeprefix("Error calling function `serialize_model`: ")}


def main() -> None:
    for text in TEXTS:
        if Path(text).is_file():
            raise AssertionError(f"{text!r} names a file that exists here")
    fixture = {
        "generator": "scripts/generate_ollama_images.py",
        "roomkit_version": roomkit.__version__,
        "ollama_version": importlib.metadata.version(ollama.__name__),
        "python_version": platform.python_version(),
        "images": [{"given": text, **made(text)} for text in TEXTS],
    }
    FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    FIXTURE.write_text(json.dumps(fixture, indent=1) + "\n", encoding="utf-8")
    print(f"Wrote {FIXTURE.relative_to(REPOSITORY)}: {len(TEXTS)} texts")


if __name__ == "__main__":
    main()
