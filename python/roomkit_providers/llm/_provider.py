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
from contextlib import contextmanager
from typing import Annotated, ClassVar

from pydantic import Field, SecretStr, TypeAdapter
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

from .. import _native
from ._failure import ProviderFailure

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
    its name, its model, what it supports, its catalogue's entry. A
    generation crosses as the contract's JSON and comes back as RoomKit's
    models; a failure is RoomKit's ``ProviderError`` or
    ``ResponseSchemaError``; the time to the first token is recorded as
    RoomKit's providers record it.

    A provider's class gives its native provider to ``__init__`` and, where
    it has a catalogue, names what writes it in ``_catalogue``.
    """

    _catalogue: ClassVar[Callable[[], str] | None] = None
    """What writes the provider's curated catalogue, as a JSON list."""

    def __init__(self, native: _native._Provider) -> None:
        self._native = native

    @property
    def name(self) -> str:
        return self._native.name

    @property
    def model_name(self) -> str:
        return self._native.model_name

    @property
    def supports_vision(self) -> bool:
        return self._native.supports_vision

    @property
    def supports_streaming(self) -> bool:
        return self._native.supports_streaming

    @property
    def supports_structured_streaming(self) -> bool:
        return self._native.supports_structured_streaming

    @property
    def supports_response_schema(self) -> bool:
        return self._native.supports_response_schema

    @property
    def supports_deferred_tools(self) -> bool:
        return self._native.supports_deferred_tools

    @property
    def supports_response_schema_with_tools(self) -> bool:
        return self._native.supports_response_schema_with_tools

    @classmethod
    def available_models(cls) -> list[ModelInfo]:
        """The provider's curated catalogue, with no request."""
        catalogue = cls._catalogue
        return [] if catalogue is None else _MODELS.validate_json(catalogue())

    async def list_models(self) -> list[ModelInfo]:
        """Every model the provider lists, filled from its catalogue where
        it knows the model."""
        with failing_as_roomkit():
            return _MODELS.validate_json(await self._native.list_models())

    def catalog_entry(self) -> ModelInfo | None:
        entry = self._native.catalog_entry_json()
        return None if entry is None else ModelInfo.model_validate_json(entry)

    @property
    def context_window(self) -> int | None:
        return self._native.context_window

    async def generate(self, context: AIContext) -> AIResponse:
        request = context.model_dump_json(include=_REQUEST)
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
        request = context.model_dump_json(include=_REQUEST)
        stream = self._native.stream(request, request_key(context))
        # The time to the first token is recorded once, at the first text
        # or reasoning, never at a call's fragment.
        started: float | None = time.monotonic()
        try:
            with failing_as_roomkit():
                while (crossed := await stream.next()) is not None:
                    for event in _EVENTS.validate_json(crossed):
                        if started is not None and isinstance(
                            event, StreamTextDelta | StreamThinkingDelta
                        ):
                            self._record_ttfb(started)
                            started = None
                        yield event
        finally:
            stream.close()

    async def close(self) -> None:
        """Release what the provider holds."""
        await self._native.close()
