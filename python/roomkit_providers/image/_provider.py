"""A RoomKit image provider over any image provider of the contract (RFC §25).

What RoomKit asks of an image provider is translated here, once, for every
image provider the native module holds: a request crosses as JSON, the
images come back as RoomKit's ``ImageResult``, a failure as RoomKit's
exception: ``ValueError`` for a request refused before anything is sent, an
``ImageGenerationError`` with every attempt and the images kept, or a
``ProviderError``.

A generation is read as a call: each attempt the provider tells crosses to
Python, is told to the host's progress callback, and the callback's outcome
crosses back, which the provider waits for. Cancelling the task that awaits
a generation stops its requests; the attempts then in flight are told to
the callback as unknown before the cancellation goes on, as RoomKit's own
providers tell them.
"""

from __future__ import annotations

import asyncio
import json
from contextlib import suppress
from typing import Any, ClassVar

from pydantic import BaseModel, TypeAdapter
from roomkit.providers.ai import AIImagePart, ModelInfo, ProviderError
from roomkit.providers.image import (
    ImageAttempt,
    ImageGenerationError,
    ImageModelInfo,
    ImageOptions,
    ImageProgressCallback,
    ImageResult,
)
from roomkit.providers.image import ImageProvider as RoomKitImageProvider

from .. import _native, _process
from ..llm._failure import failing_as_roomkit
from ._config import Config, Veiled

_RESULTS: TypeAdapter[list[ImageResult]] = TypeAdapter(list[ImageResult])

# The most images a request asks that crosses: the contract counts them in
# 32 bits.
_MOST_IMAGES = 2**32 - 1


def _entry(written: dict[str, Any]) -> ModelInfo:
    """A catalogue's entry: an image model's, with its capabilities, or a
    model's."""
    return (
        ImageModelInfo.model_validate(written)
        if "image" in written
        else ModelInfo.model_validate(written)
    )


def _part(part: AIImagePart) -> dict[str, Any]:
    return {"url": part.url, "mime_type": part.mime_type}


class _Generation:
    """One generation as Python reads it: the attempts told, then its
    outcome."""

    def __init__(self, call: _native._ImageCall, callback: ImageProgressCallback | None) -> None:
        self._call = call
        self._callback = callback
        # The failure of the host's callback, which a generation it failed
        # is raised from.
        self._cause: Exception | None = None

    async def _next(self) -> dict[str, Any] | None:
        _process.enter()
        with failing_as_roomkit():
            crossed = await self._call.next()
        return None if crossed is None else json.loads(crossed)

    async def _tell(self, item: dict[str, Any]) -> None:
        """Tells the host's callback the attempt `item` holds, and answers
        the provider with how it went."""
        failed: str | None = None
        if self._callback is not None:
            try:
                await self._callback(ImageAttempt.model_validate(item["told"]))
            except Exception as exc:
                self._cause = exc
                failed = str(exc)
        self._call.reply(item["token"], failed)

    async def outcome(self, label: str) -> list[ImageResult]:
        while True:
            item = await self._next()
            if item is None:
                raise ProviderError(
                    "the provider's generation ended without its outcome",
                    retryable=False,
                    provider=label,
                )
            if "told" in item:
                await self._tell(item)
                continue
            return self._ended(item)

    def _ended(self, item: dict[str, Any]) -> list[ImageResult]:
        if "images" in item:
            return _RESULTS.validate_python(item["images"])
        if "refused" in item:
            raise ValueError(item["refused"])
        if "failed" in item:
            failed = item["failed"]
            attempts = [ImageAttempt.model_validate(attempt) for attempt in failed["attempts"]]
            # Raised from the host's callback where its failure is the one
            # that failed the generation, as RoomKit's provider raises it.
            cause = self._cause
            by_the_callback = f"Image progress callback failed: {cause}"
            raise ImageGenerationError(
                failed["message"], provider=failed["provider"], attempts=attempts
            ) from (cause if cause is not None and by_the_callback in failed["message"] else None)
        error = item["error"]
        raise ProviderError(
            error["message"],
            retryable=error["retryable"],
            provider=error["provider"],
            status_code=error["status_code"],
            context_overflow=error["context_overflow"],
            before_request=error["before_request"],
        )

    async def cancelled(self) -> None:
        """Stops the generation, then tells the host's callback each attempt
        it left in flight, unknown: the vendor may still bill it."""
        if _process.forked():
            return
        self._call.close()
        while (item := await self._next()) is not None:
            if "told" in item:
                await self._tell(item)

    def close(self) -> None:
        if not _process.forked():
            self._call.close()


class Provider[C: BaseModel](RoomKitImageProvider):
    """A RoomKit image provider over an image provider the native module
    holds.

    Its name, its model, whether it edits and its catalogue's entry come
    from the native provider, read once, when it is built. A vendor's class
    is one statement, which names its configuration::

        class Vendor(Provider[VendorConfig | RoomKitVendorConfig], config=VendorConfig):
            ...
    """

    _configured_by: ClassVar[type[Config]]
    """The provider's configuration, which names its vendor."""

    _catalogue: ClassVar[str | None] = None
    """The provider's curated catalogue, as a JSON list, read once when the
    class is defined."""

    def __init_subclass__(cls, *, config: type[Config] | None = None, **kwargs: Any) -> None:
        super().__init_subclass__(**kwargs)
        if config is None:
            if not hasattr(cls, "_configured_by"):
                raise TypeError("an image provider's class names its configuration (config=...)")
            return
        cls._configured_by = config
        _process.enter()
        with failing_as_roomkit():
            cls._catalogue = _native.image_catalogue(config._vendor)

    def __init__(self, config: C) -> None:
        # From here on no frame holds the configuration as it shows itself.
        config = Veiled(config, type(self)._configured_by)
        if not config.taken():
            raise TypeError(
                "config must be the provider's own configuration, or RoomKit's of the same "
                "provider: one derived from either belongs to another provider"
            )
        _process.enter()
        with failing_as_roomkit():
            native = config.native_image()
        self._config = config.held()
        self._hold(native)

    def _hold(self, native: _native._ImageProvider) -> None:
        """Hold `native`, and what it says of itself."""
        self._native = native
        self._name = native.name
        self._label = native.label
        self._model_name = native.model_name
        self._supports_editing = native.supports_editing
        entry = native.catalog_entry_json()
        self._catalog_entry = None if entry is None else _entry(json.loads(entry))

    @property
    def name(self) -> str:
        return self._name

    @property
    def model_name(self) -> str:
        return self._model_name

    @property
    def supports_editing(self) -> bool:
        return self._supports_editing

    @classmethod
    def available_models(cls) -> list[ModelInfo]:
        """The provider's curated catalogue, with no request."""
        catalogue = cls._catalogue
        return [] if catalogue is None else [_entry(entry) for entry in json.loads(catalogue)]

    def catalog_entry(self) -> ModelInfo | None:
        return self._catalog_entry

    async def generate(
        self,
        prompt: str,
        *,
        size: str | None = None,
        n: int = 1,
        reference_images: list[AIImagePart] | None = None,
    ) -> list[ImageResult]:
        asked = self._asked("generate", prompt, size, n, reference_images)
        return await self._generated(asked, None)

    async def generate_with_options(
        self,
        prompt: str,
        *,
        size: str | None = None,
        n: int = 1,
        reference_images: list[AIImagePart] | None = None,
        options: ImageOptions | None = None,
        mask: AIImagePart | None = None,
        on_progress: ImageProgressCallback | None = None,
    ) -> list[ImageResult]:
        asked = self._asked("generate_with_options", prompt, size, n, reference_images)
        asked["options"] = (
            None if options is None else options.model_dump(mode="json", exclude_none=True)
        )
        asked["mask"] = None if mask is None else _part(mask)
        asked["told"] = on_progress is not None
        return await self._generated(asked, on_progress)

    @staticmethod
    def _asked(
        call: str,
        prompt: str,
        size: str | None,
        n: int,
        reference_images: list[AIImagePart] | None,
    ) -> dict[str, Any]:
        """A request as it crosses: a count below one is none, which the
        provider refuses as RoomKit's does."""
        return {
            "call": call,
            "prompt": prompt,
            "size": size,
            "n": min(max(n, 0), _MOST_IMAGES),
            "reference_images": [_part(part) for part in reference_images or []],
            "options": None,
            "mask": None,
            "told": False,
        }

    async def _generated(
        self, asked: dict[str, Any], callback: ImageProgressCallback | None
    ) -> list[ImageResult]:
        _process.start()
        with failing_as_roomkit():
            call = self._native.generation(json.dumps(asked))
        generation = _Generation(call, callback)
        try:
            return await generation.outcome(self._label)
        except asyncio.CancelledError:
            await generation.cancelled()
            raise
        finally:
            generation.close()

    async def close(self) -> None:
        """Release what the provider holds. It never raises."""
        if _process.forked():
            return
        _process.start()
        with suppress(RuntimeError):
            await self._native.close()
