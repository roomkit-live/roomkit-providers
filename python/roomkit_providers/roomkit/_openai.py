"""RoomKit's ``AIProvider`` over the Rust OpenAI provider."""

from __future__ import annotations

import time
from collections.abc import AsyncIterator, Iterator
from contextlib import contextmanager
from dataclasses import fields
from typing import Annotated

from pydantic import Field, TypeAdapter
from roomkit.providers.ai import (
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
from roomkit.providers.openai import OpenAIConfig as RoomKitOpenAIConfig

from .. import _errors, _native
from ..openai import OpenAIConfig

# What a provider reads of a context: the members of the contract's request.
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
    "metadata",
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


def _native_config(config: OpenAIConfig | RoomKitOpenAIConfig) -> OpenAIConfig:
    """`config` as the native provider is built from it."""
    if isinstance(config, OpenAIConfig):
        return config
    if type(config) is not RoomKitOpenAIConfig:
        raise TypeError(
            "config must be RoomKit's OpenAIConfig or roomkit_providers.openai.OpenAIConfig: "
            "the configuration of a provider derived from OpenAI's belongs to that provider"
        )
    members = {field.name: getattr(config, field.name) for field in fields(OpenAIConfig)}
    members["api_key"] = config.api_key.get_secret_value()
    return OpenAIConfig(**members)


def _as_roomkit(error: _errors.ProviderError) -> ProviderError:
    """`error` as RoomKit's own exception, with the same attributes."""
    if isinstance(error, _errors.ResponseSchemaError):
        return ResponseSchemaError(error.args[0], reason=error.reason, provider=error.provider)
    return ProviderError(
        error.args[0],
        retryable=error.retryable,
        provider=error.provider,
        status_code=error.status_code,
        context_overflow=error.context_overflow,
        before_request=error.before_request,
    )


@contextmanager
def _failing_as_roomkit() -> Iterator[None]:
    """Raises a failure of the native provider as RoomKit's exception."""
    try:
        yield
    except _errors.ProviderError as error:
        raise _as_roomkit(error) from error


class OpenAIAIProvider(AIProvider):
    """RoomKit's OpenAI provider, implemented in Rust.

    It takes the place of ``roomkit.providers.openai.OpenAIAIProvider`` in a
    RoomKit agent: it is an ``AIProvider``, takes an ``AIContext``, gives
    RoomKit's models and raises RoomKit's ``ProviderError``. What it sends
    and reads is the Rust provider's.

    There is no ``transport`` parameter: the provider builds its own HTTP
    client. ``config`` is RoomKit's ``OpenAIConfig``, or this package's.
    """

    def __init__(self, config: RoomKitOpenAIConfig | OpenAIConfig) -> None:
        native = _native_config(config)
        self._config = config
        with _failing_as_roomkit():
            self._native = _native._OpenAIProvider(native._form(), native.api_key)

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
    def supports_response_schema_with_tools(self) -> bool:
        return self._native.supports_response_schema_with_tools

    @property
    def supports_deferred_tools(self) -> bool:
        return self._native.supports_deferred_tools

    @classmethod
    def available_models(cls) -> list[ModelInfo]:
        """The curated catalogue of OpenAI's models, with no request."""
        return _MODELS.validate_json(_native._OpenAIProvider.available_models_json())

    async def list_models(self) -> list[ModelInfo]:
        """Every model the endpoint lists, filled from the catalogue where
        it knows the model."""
        with _failing_as_roomkit():
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
        with _failing_as_roomkit():
            answered = await self._native.generate(request)
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
        stream = self._native.stream(context.model_dump_json(include=_REQUEST), False)
        # The time to the first token is recorded once, at the first text
        # or reasoning, never at a call's fragment.
        started: float | None = time.monotonic()
        try:
            with _failing_as_roomkit():
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
