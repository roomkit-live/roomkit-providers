"""A RoomKit provider over any provider of the contract.

What RoomKit asks of a provider is translated here, once, for every provider
the native module holds: a context becomes the contract's request, an answer
RoomKit's models, a failure RoomKit's exception. So is how a provider is
built from its configuration, and what guards each entry into the native
module. A provider's own module adds the members of its configuration and
one class statement, and nothing of what follows.
"""

from __future__ import annotations

import time
from collections.abc import AsyncIterator
from contextlib import suppress
from typing import Annotated, Any, ClassVar, Self

from pydantic import BaseModel, Field, SecretStr, TypeAdapter
from pydantic_core import PydanticSerializationError
from roomkit.providers.ai import (
    API_KEY_METADATA_KEY,
    AIContext,
    AIProvider,
    AIResponse,
    ModelInfo,
    ProviderError,
    StreamDone,
    StreamEvent,
    StreamTextDelta,
    StreamThinkingDelta,
    StreamToolCall,
    StreamToolCallDelta,
    StreamUsage,
)

from .. import _native, _process
from ._config import Config, Veiled
from ._failure import failing_as_roomkit

# The most characters of a refusal's words a failure repeats.
_SAID_LIMIT = 500

# The fewest characters of a key that is removed from a failure's words: a
# shorter one would be found in ordinary words. And what stands in its place.
_SECRET_MIN = 8
_MARK = "[***]"

# What crosses of a context: the members of the contract's request, but its
# metadata. No provider reads anything of a context's metadata but the
# credential of one request, which crosses apart; and a host may leave there
# what has no JSON form.
_REQUEST = {
    "messages",
    "system_prompt",
    "temperature",
    "max_tokens",
    "thinking_budget",
    "enable_thinking",
    "reasoning_effort",
    "tools",
    "response_schema",
}

# The events one crossing of a stream brings back, read as RoomKit's models
# by their ``type``.
_Event = Annotated[
    StreamThinkingDelta
    | StreamTextDelta
    | StreamToolCallDelta
    | StreamToolCall
    | StreamUsage
    | StreamDone,
    Field(discriminator="type"),
]
_EVENTS: TypeAdapter[list[StreamEvent]] = TypeAdapter(list[_Event])
_MODELS: TypeAdapter[list[ModelInfo]] = TypeAdapter(list[ModelInfo])


def request_key(context: AIContext) -> str | None:
    """The credential the context's host lent this one request, or None for
    the provider's own: text that is not empty, as RoomKit's providers read
    it. It never crosses inside the request's JSON, where RoomKit writes it
    masked."""
    value = context.metadata.get(API_KEY_METADATA_KEY)
    if isinstance(value, SecretStr):
        value = value.get_secret_value()
    return value if isinstance(value, str) and value else None


def _without(said: str, lent: str | None) -> str:
    """`said` without `lent`, the key a host lent one request."""
    return said.replace(lent, _MARK) if lent and len(lent) >= _SECRET_MIN else said


def _is_output(event: StreamEvent) -> bool:
    """Whether `event` is the first a reader sees of an answer: text or
    reasoning that is not empty. A signature alone, a redacted block and a
    fragment of a call are none."""
    if isinstance(event, StreamTextDelta):
        return bool(event.text)
    return isinstance(event, StreamThinkingDelta) and bool(event.thinking)


def _holds_output(response: AIResponse) -> bool:
    """Whether a whole answer holds text or reasoning: one made of calls
    alone, or of redacted reasoning, holds neither."""
    parts = response.thinking_parts or ()
    return bool(response.content or response.thinking or any(part.thinking for part in parts))


class Provider[C: BaseModel](AIProvider):
    """A RoomKit provider over a provider the native module holds.

    Every answer RoomKit reads on a provider comes from the native provider:
    its name, its model, what it supports, its catalogue's entry. They are
    read once, when the provider is built. A generation crosses as the
    contract's JSON and comes back as RoomKit's models; a failure is
    RoomKit's ``ProviderError`` or ``ResponseSchemaError``; the time to the
    first token is recorded once, by one rule.

    A provider's class is one statement, which names its configuration, and
    a docstring::

        class Vendor(Provider[VendorConfig | RoomKitVendorConfig], config=VendorConfig):
            ...

    It writes no ``__init__``: the provider is built here, from its
    configuration alone, and every entry into the native module goes through
    this class.
    """

    _configured_by: ClassVar[type[Config]]
    """The provider's configuration, which names its vendor."""

    _catalogue: ClassVar[str | None] = None
    """The provider's curated catalogue, as a JSON list, read once when the
    class is defined: reading it later asks nothing of the native module."""

    def __init_subclass__(cls, *, config: type[Config] | None = None, **kwargs: Any) -> None:
        super().__init_subclass__(**kwargs)
        if config is None:
            # A class derived from a provider's is that provider's.
            if not hasattr(cls, "_configured_by"):
                raise TypeError("a provider's class names its configuration (config=...)")
            return
        cls._configured_by = config
        _process.enter()
        with failing_as_roomkit():
            cls._catalogue = _native.catalogue(config._vendor)

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
            native = config.native()
        self._config = config.held()
        self._hold(native)

    @classmethod
    def _over(cls, native: _native._Provider) -> Self:
        """A provider over a native provider already built: for the tests of
        the crossing, which script one."""
        provider = cls.__new__(cls)
        provider._hold(native)
        return provider

    def _hold(self, native: _native._Provider) -> None:
        """Hold `native`, and what it says of itself."""
        self._native = native
        self._name = native.name
        # What the provider's failures carry as their ``provider``.
        self._label = native.label
        self._model_name = native.model_name
        self._supports_vision = native.supports_vision
        self._supports_streaming = native.supports_streaming
        self._supports_structured_streaming = native.supports_structured_streaming
        self._supports_response_schema = native.supports_response_schema
        self._supports_deferred_tools = native.supports_deferred_tools
        self._supports_response_schema_with_tools = native.supports_response_schema_with_tools
        self._context_window = native.context_window
        entry = native.catalog_entry_json()
        self._catalog_entry = None if entry is None else ModelInfo.model_validate_json(entry)

    @property
    def name(self) -> str:
        return self._name

    @property
    def model_name(self) -> str:
        return self._model_name

    @property
    def supports_vision(self) -> bool:
        return self._supports_vision

    @property
    def supports_streaming(self) -> bool:
        return self._supports_streaming

    @property
    def supports_structured_streaming(self) -> bool:
        return self._supports_structured_streaming

    @property
    def supports_response_schema(self) -> bool:
        return self._supports_response_schema

    @property
    def supports_deferred_tools(self) -> bool:
        return self._supports_deferred_tools

    @property
    def supports_response_schema_with_tools(self) -> bool:
        return self._supports_response_schema_with_tools

    @classmethod
    def available_models(cls) -> list[ModelInfo]:
        """The provider's curated catalogue, with no request."""
        catalogue = cls._catalogue
        return [] if catalogue is None else _MODELS.validate_json(catalogue)

    async def list_models(self) -> list[ModelInfo]:
        """Every model the provider lists, filled from its catalogue where
        it knows the model."""
        _process.start()
        with failing_as_roomkit():
            return _MODELS.validate_json(await self._native.list_models())

    def catalog_entry(self) -> ModelInfo | None:
        return self._catalog_entry

    @property
    def context_window(self) -> int | None:
        return self._context_window

    def _request(self, context: AIContext) -> str:
        """`context` as the contract's request, in JSON. One that has no
        JSON form is refused as a provider refuses a request before it
        sends it, in words that hold no key the context lends."""
        try:
            return context.model_dump_json(include=_REQUEST)
        except PydanticSerializationError as refused:
            said = _without(f"the request is refused: {refused}", request_key(context))
            raise ProviderError(
                said[:_SAID_LIMIT], retryable=False, provider=self._label, before_request=True
            ) from None

    async def generate(self, context: AIContext) -> AIResponse:
        request = self._request(context)
        _process.start()
        started = time.monotonic()
        with failing_as_roomkit():
            answered = await self._native.generate(request, request_key(context))
        response = AIResponse.model_validate_json(answered)
        # The time to the whole answer is the time to its first token, for
        # an answer that holds text or reasoning: one of calls alone has
        # none.
        if _holds_output(response):
            self._record_ttfb(started)
        return response

    async def generate_stream(self, context: AIContext) -> AsyncIterator[str]:
        """Yield text deltas (thinking content filtered out)."""
        async for event in self.generate_structured_stream(context):
            if isinstance(event, StreamTextDelta):
                yield event.text

    async def generate_structured_stream(self, context: AIContext) -> AsyncIterator[StreamEvent]:
        """Yield the events of one generation as they come: reasoning and
        text, each call as it is written, then the whole calls and the done
        event. A response schema is checked before the done event."""
        request = self._request(context)
        _process.start()
        stream = self._native.stream(request, request_key(context))
        # The time to the first token is recorded once, at the first text
        # or reasoning that is not empty: never at a signature alone, a
        # redacted block or a call's fragment.
        started: float | None = time.monotonic()
        try:
            with failing_as_roomkit():
                while True:
                    _process.enter()
                    crossed = await stream.next()
                    if crossed is None:
                        break
                    for event in _EVENTS.validate_json(crossed):
                        if started is not None and _is_output(event):
                            self._record_ttfb(started)
                            started = None
                        yield event
        finally:
            # A forked child leaves its parent's stream as it is.
            if not _process.forked():
                stream.close()

    async def close(self) -> None:
        """Release what the provider holds. It never raises: where nothing
        can be released any more, in a forked child or once the interpreter
        is exiting, it does nothing."""
        if _process.forked():
            return
        _process.start()
        with suppress(RuntimeError):
            await self._native.close()
