"""A RoomKit provider over any provider of the contract.

What RoomKit asks of a provider is translated here, once, for every provider
the native module holds: a context becomes the contract's request, an answer
RoomKit's models, a failure RoomKit's exception. A provider's own module
adds its configuration, how it is built and its catalogue, and nothing of
what follows.
"""

from __future__ import annotations

import time
from collections.abc import AsyncIterator, Callable, Iterator
from contextlib import contextmanager, suppress
from typing import Annotated, ClassVar

from pydantic import Field, SecretStr, TypeAdapter
from pydantic_core import PydanticSerializationError
from roomkit.providers.ai import (
    API_KEY_METADATA_KEY,
    AIContext,
    AIProvider,
    AIResponse,
    ModelInfo,
    ProviderError,
    ResponseSchemaError,
    StreamDone,
    StreamEvent,
    StreamTextDelta,
    StreamThinkingDelta,
    StreamToolCall,
    StreamToolCallDelta,
    StreamUsage,
)

from .. import _native, _process
from ._failure import ProviderFailure

# The most characters of a refusal's words a failure repeats.
_SAID_LIMIT = 500

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


def _as_roomkit(failure: ProviderFailure) -> ProviderError:
    """`failure` as RoomKit's own exception, with the same attributes."""
    if failure.schema_reason is not None:
        return ResponseSchemaError(
            failure.args[0], reason=failure.schema_reason, provider=failure.provider
        )
    return ProviderError(
        failure.args[0],
        retryable=failure.retryable,
        provider=failure.provider,
        status_code=failure.status_code,
        context_overflow=failure.context_overflow,
        before_request=failure.before_request,
    )


@contextmanager
def failing_as_roomkit() -> Iterator[None]:
    """Raises a failure of the native module as RoomKit's exception."""
    try:
        yield
    except ProviderFailure as failure:
        raise _as_roomkit(failure) from None


class Provider(AIProvider):
    """A RoomKit provider over a provider the native module holds.

    Every answer RoomKit reads on a provider comes from the native provider:
    its name, its model, what it supports, its catalogue's entry. They are
    read once, when the provider is built. A generation crosses as the
    contract's JSON and comes back as RoomKit's models; a failure is
    RoomKit's ``ProviderError`` or ``ResponseSchemaError``; the time to the
    first token is recorded as RoomKit's providers record it.

    A provider's class gives its native provider to ``__init__`` and, where
    it has a catalogue, names what writes it in ``_catalogue``.
    """

    _catalogue: ClassVar[Callable[[], str] | None] = None
    """What writes the provider's curated catalogue, as a JSON list."""

    def __init__(self, native: _native._Provider) -> None:
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
        if catalogue is None:
            return []
        _process.enter()
        return _MODELS.validate_json(catalogue())

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
        sends it."""
        try:
            return context.model_dump_json(include=_REQUEST)
        except PydanticSerializationError as refused:
            said = f"the request is refused: {refused}"[:_SAID_LIMIT]
            raise ProviderError(
                said, retryable=False, provider=self._label, before_request=True
            ) from None

    async def generate(self, context: AIContext) -> AIResponse:
        request = self._request(context)
        _process.start()
        started = time.monotonic()
        with failing_as_roomkit():
            answered = await self._native.generate(request, request_key(context))
        # The answer came: the time to it is the time to its first token.
        self._record_ttfb(started)
        return AIResponse.model_validate_json(answered)

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
        # or reasoning, never at a call's fragment.
        started: float | None = time.monotonic()
        try:
            with failing_as_roomkit():
                while True:
                    _process.enter()
                    crossed = await stream.next()
                    if crossed is None:
                        break
                    for event in _EVENTS.validate_json(crossed):
                        if started is not None and isinstance(
                            event, StreamTextDelta | StreamThinkingDelta
                        ):
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
