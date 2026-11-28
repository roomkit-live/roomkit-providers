"""The replay of an image provider's recorded cases through the Python
package: what the package's image provider sent and gave, and where that
differs from what RoomKit's own provider did.

A case is asked as a host asks it: its configuration is the package's, the
call RoomKit's ``generate`` or ``generate_with_options``, with a progress
callback that records each attempt it is told of; what the provider gives
is RoomKit's models, written back in the JSON form the fixture holds, and a
failure RoomKit's own exception.

The comparison is the Rust replay's (``harness::images`` of
``crates/roomkit-providers-client``), in its words: the requests, the
images, the attempts told, and a failure by what its case says is compared.
"""

from __future__ import annotations

import copy
import json
from collections import Counter
from dataclasses import dataclass, field
from typing import Any

from fake_endpoint import Fake, Seen
from pydantic import BaseModel
from replay_cases import (
    CLOCK,
    ROOT,
    SHORT_TIMEOUTS,
    _requests_differ,
    _text,
    reply_of,
    said,
    set_apart,
)
from roomkit.providers.ai import AIImagePart, ProviderError
from roomkit.providers.image import ImageAttempt, ImageGenerationError, ImageOptions
from roomkit_providers import _native

IMAGE_GENERATE, IMAGE_WITH_OPTIONS = "image_generate", "image_generate_with_options"


@dataclass(frozen=True)
class ImageReplays:
    """An image provider the replay drives, as its test module states it."""

    # The name the native module holds the provider under, among image
    # vendors, which the name of its crate ends with.
    vendor: str
    provider: type
    config: type[BaseModel]
    # The cases the Rust replay names, with the start of each difference
    # it states.
    named: dict[str, list[str]] = field(default_factory=dict)

    def fixture(self) -> dict[str, Any]:
        fixtures = ROOT / f"crates/roomkit-providers-{self.vendor}/tests/fixtures"
        return json.loads((fixtures / "python_image_cases.json").read_text(encoding="utf-8"))


@dataclass
class Replayed:
    """What replaying an image case gave."""

    key: str = ""
    unbuilt: Exception | None = None
    images: Any = None
    error: Exception | None = None
    told: list[dict[str, Any]] = field(default_factory=list)
    seen: list[Seen] = field(default_factory=list)
    # As the shared request comparison reads a call.
    request_key: str | None = None


def _callback(progress: str | None, told: list[dict[str, Any]]) -> Any:
    """The callback a case asks: none, one that records each attempt it is
    told of, or one that records it, then fails when told of a succeeded
    one."""
    if progress is None:
        return None

    async def recorded(attempt: ImageAttempt) -> None:
        told.append(attempt.model_dump(mode="json"))
        if progress == "fail" and attempt.status == "succeeded":
            raise RuntimeError("storage unavailable")

    return recorded


async def _called(provider: Any, case: dict[str, Any], replayed: Replayed) -> None:
    asked = case["request"]
    references = [AIImagePart.model_validate(part) for part in asked["reference_images"]]
    arguments: dict[str, Any] = {
        "size": asked["size"],
        "n": asked["n"],
        "reference_images": references,
    }
    if case["call"] == IMAGE_WITH_OPTIONS:
        options, mask = asked["options"], asked["mask"]
        arguments["options"] = None if options is None else ImageOptions(**options)
        arguments["mask"] = None if mask is None else AIImagePart.model_validate(mask)
        arguments["on_progress"] = _callback(asked["progress"], replayed.told)
        entry = provider.generate_with_options
    else:
        entry = provider.generate
    try:
        images = await entry(asked["prompt"], **arguments)
        replayed.images = [image.model_dump(mode="json") for image in images]
    except (ProviderError, ValueError) as error:
        replayed.error = error


async def replay(replays: ImageReplays, case: dict[str, Any], fake: Fake) -> Replayed:
    """Replays `case`: its provider built from its configuration, its
    requests sent to `fake`, which answers with the case's exchanges."""
    config = dict(case["config"])
    short = SHORT_TIMEOUTS.get(set_apart(case))
    if short is not None:
        config["timeout"] = short
    replayed = Replayed(key=config.get("api_key") or "")
    try:
        provider = replays.provider(replays.config(**config))
    except ProviderError as error:
        replayed.unbuilt = error
        return replayed
    built = provider._config
    key = getattr(built, type(built)._key, None)
    native, _waits = _native._image_replaying(
        replays.vendor, built.model_dump_json(), key, "127.0.0.1", fake.port, CLOCK
    )
    provider._hold(native)
    fake.answer(*(reply_of(exchange["response"]) for exchange in case["exchanges"]))
    await _called(provider, case, replayed)
    del provider
    await fake.served()
    replayed.seen = list(fake.seen)
    return replayed


def _name_ids(value: Any, named: dict[str, str]) -> Any:
    """`value` with each attempt's id named by its place, as the Rust replay
    names it: Python and the provider draw them at random."""
    if isinstance(value, list):
        return [_name_ids(item, named) for item in value]
    if not isinstance(value, dict):
        return value
    written = {}
    for key, member in value.items():
        if key in ("id", "attempt_id") and isinstance(member, str):
            written[key] = named.setdefault(member, f"<attempt {len(named)}>")
        else:
            written[key] = _name_ids(member, named)
    return written


def _unworded(value: Any) -> Any:
    """`value` with the words of each attempt's error left out: whether the
    attempt has an error is still compared."""
    if isinstance(value, list):
        return [_unworded(item) for item in value]
    if not isinstance(value, dict):
        return value
    worded = "status" in value and isinstance(value.get("error"), str)
    written = {key: _unworded(member) for key, member in value.items()}
    if worded:
        written["error"] = "<an error>"
    return written


def _values(case: dict[str, Any], replayed: Replayed) -> tuple[list[Any], list[Any]]:
    """What Python gave and what the provider gave, in the same order: the
    attempts told, each attempt's in its order, the images, the attempts and
    the images of a failed generation."""
    expected, failure = case["expected"], case["expected"].get("error") or {}
    error = replayed.error
    generation = error if isinstance(error, ImageGenerationError) else None
    by_index = lambda told: sorted(told, key=lambda attempt: attempt.get("index", 0))  # noqa: E731
    expected_values = [
        by_index(expected.get("progress") or []),
        expected.get("results"),
        failure.get("attempts"),
        failure.get("results"),
    ]
    given_values = [
        by_index(replayed.told),
        replayed.images,
        None if generation is None else [a.model_dump(mode="json") for a in generation.attempts],
        None if generation is None else [r.model_dump(mode="json") for r in generation.results],
    ]
    expected_values = _name_ids(expected_values, {})
    given_values = _name_ids(given_values, {})
    compare = case.get("compare") or {}
    if compare.get("message") not in ("roomkit", "server"):
        expected_values, given_values = _unworded(expected_values), _unworded(given_values)
    return expected_values, given_values


def _class_of(error: Exception) -> str:
    if isinstance(error, ImageGenerationError):
        return "ImageGenerationError"
    return "ProviderError" if isinstance(error, ProviderError) else "ValueError"


def _failure_differs(
    case: dict[str, Any], error: Exception, outcomes: list[tuple[Any, Any]], found: list[str]
) -> None:
    failure, compare = case["expected"]["error"], case.get("compare")
    if compare is None:
        found.append("a failure with no rule to compare it")
        return
    if not compare["exactly"]:
        found.append(f"a {failure['class']} no provider reproduces")
        return
    given: Any = error
    same = {
        "class": lambda: failure["class"] == _class_of(error),
        "status_code": lambda: failure.get("status_code") == given.status_code,
        "retryable": lambda: failure.get("retryable") == given.retryable,
        "provider": lambda: failure.get("provider") == given.provider,
        "context_overflow": lambda: failure.get("context_overflow") == given.context_overflow,
        "before_request": lambda: failure.get("before_request") == given.before_request,
        "attempts": lambda: outcomes[0][0] == outcomes[0][1],
        "results": lambda: outcomes[1][0] == outcomes[1][1],
    }
    for attribute in compare["exactly"]:
        try:
            matched = same[attribute]()
        except AttributeError:
            matched = False
        if not matched:
            found.append(f"the failure's {attribute} differs: {error!r}")
    message = str(error) if isinstance(error, ValueError) else said(error)
    if compare["message"] in ("roomkit", "server") and message != failure["message"]:
        found.append(f"the failure says {message!r}")


def differences(case: dict[str, Any], replayed: Replayed) -> list[str]:
    """Every way `replayed` differs from what Python did of `case`."""
    if replayed.unbuilt is not None:
        return [f"the provider is not built: {said(replayed.unbuilt)}"]
    found: list[str] = []
    _requests_differ(case, replayed.key, replayed, found)  # type: ignore[arg-type]
    expected, given = _values(case, replayed)
    failure = case["expected"].get("error")
    if case["expected"].get("progress") is not None and _text(given[0]) != _text(expected[0]):
        found.append(f"the attempts told: {_text(given[0])}")
    error = replayed.error
    if error is None and failure is None:
        if _text(given[1]) != _text(expected[1]):
            found.append(f"the images: {_text(given[1])}")
    elif error is None:
        found.append(f"images where Python fails: {failure['message']}")
    elif failure is None:
        found.append(f"a failure where Python answers: {error!r}")
    else:
        _failure_differs(case, error, [(expected[2], given[2]), (expected[3], given[3])], found)
    return found


async def every_case(replays: ImageReplays) -> tuple[dict[tuple[str, str], int], list[str]]:
    """Replays every case of the fixture on one fake. Gives how many cases
    of each call replay as Python or as the Rust replay names them, and
    what is wrong, case by case."""
    counts: Counter[tuple[str, str]] = Counter()
    wrong: list[str] = []
    async with Fake() as fake:
        for case in replays.fixture()["cases"]:
            replayed = await replay(replays, copy.deepcopy(case), fake)
            found = differences(case, replayed)
            stated = replays.named.get(case["name"])
            if stated is None:
                if found:
                    wrong.append(f"{case['name']}: {'; '.join(found)}")
                counts[case["call"], "as Python"] += 1
                continue
            as_stated = len(found) == len(stated) and all(
                difference.startswith(prefix)
                for difference, prefix in zip(found, stated, strict=True)
            )
            if not as_stated:
                wrong.append(f"{case['name']}: named as {stated}, differs as {found}")
            counts[case["call"], "named"] += 1
    return dict(counts), wrong


async def made_wrong(replays: ImageReplays, name: str, wrong: Any) -> list[str]:
    """What the replay says of the case `name` once `wrong` changed what was
    replayed: the case must first replay as Python."""
    case = next(case for case in replays.fixture()["cases"] if case["name"] == name)
    async with Fake() as fake:
        replayed = await replay(replays, copy.deepcopy(case), fake)
    found = differences(case, replayed)
    if found:
        raise AssertionError(f"{name} does not replay as Python: {found}")
    wrong(replayed)
    return differences(case, replayed)
