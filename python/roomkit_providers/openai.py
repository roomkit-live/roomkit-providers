"""The OpenAI provider: Chat Completions, whole and streamed, without RoomKit."""

from __future__ import annotations

import copy
import json
from collections.abc import AsyncIterator
from dataclasses import dataclass, fields
from typing import Any

from . import _native
from ._contract import AIRequest, AIResponse, ModelInfo, StreamEvent

__all__ = ["OpenAIAIProvider", "OpenAIConfig"]

# What stands for a key wherever a configuration is written or shown.
_MASK = "**********"
# The switches a modern model decides on OpenAI's own endpoint when the
# caller leaves them unset.
_DECIDED = ("use_max_completion_tokens", "supports_custom_temperature")


def _dumps(value: Any) -> str:
    """``value`` as the JSON text that crosses to the native module."""
    return json.dumps(value, allow_nan=False)


@dataclass(init=False, frozen=True, slots=True, repr=False)
class OpenAIConfig:
    """What an OpenAI provider is configured with: the fields and the
    defaults of RoomKit's ``OpenAIConfig``.

    A configuration is checked when it is built, by the reader of its JSON
    form: a value that does not fit is a ``ValueError``. An argument it does
    not know is ignored, as RoomKit's configuration ignores one. It cannot
    be changed once built.

    Attributes:
        api_key: The key, sent as a bearer token. An empty key sends no
            ``Authorization``, for a local server that asks for none. It is
            never shown by ``repr``.
        base_url: The base URL of an OpenAI-compatible server; ``None`` is
            OpenAI's own endpoint.
        model: The model to ask.
        max_tokens: The most tokens an answer holds; ``None`` sends no cap.
        timeout: The longest wait, in seconds, for the head of an answer,
            then between two reads of its body.
        connect_timeout: The longest wait, in seconds, for a connection.
        max_retries: How many times a request is tried again.
        include_stream_usage: Whether a stream is asked for its usage.
        use_max_completion_tokens: Whether the cap goes as
            ``max_completion_tokens``. Left unset (``None``), it is decided
            from the model on OpenAI's own endpoint, and read back decided.
        supports_custom_temperature: Whether a temperature is sent. Left
            unset (``None``), it is decided as the switch above is.
        reasoning_effort: How hard a reasoning model thinks, unless the
            request says.
        default_headers: Headers added to every request. Their values are
            never shown by ``repr``.
        extra_body: Members merged into the body of every request.
        supports_response_schema: Whether the server honours a response
            schema; ``None`` keeps the provider's default.
        supports_response_schema_with_tools: Whether it honours one beside
            tools; ``None`` keeps the provider's default.
        supports_reasoning_effort_with_tools: Whether it takes a reasoning
            effort beside tools, for a model the provider cannot know.
        thinking_field: The field of an assistant message the server reads a
            model's earlier reasoning from; ``None`` sends none.
    """

    api_key: str
    base_url: str | None = None
    model: str
    max_tokens: int | None = 1024
    timeout: float = 30.0
    connect_timeout: float = 5.0
    max_retries: int = 0
    include_stream_usage: bool = True
    use_max_completion_tokens: bool | None = None
    supports_custom_temperature: bool | None = None
    reasoning_effort: str | None = None
    default_headers: dict[str, str] | None = None
    extra_body: dict[str, Any] | None = None
    supports_response_schema: bool | None = None
    supports_response_schema_with_tools: bool | None = None
    supports_reasoning_effort_with_tools: bool | None = None
    thinking_field: str | None = None

    def __init__(
        self,
        *,
        api_key: str,
        base_url: str | None = None,
        model: str,
        max_tokens: int | None = 1024,
        timeout: float = 30.0,
        connect_timeout: float = 5.0,
        max_retries: int = 0,
        include_stream_usage: bool = True,
        use_max_completion_tokens: bool | None = None,
        supports_custom_temperature: bool | None = None,
        reasoning_effort: str | None = None,
        default_headers: dict[str, str] | None = None,
        extra_body: dict[str, Any] | None = None,
        supports_response_schema: bool | None = None,
        supports_response_schema_with_tools: bool | None = None,
        supports_reasoning_effort_with_tools: bool | None = None,
        thinking_field: str | None = None,
        **unknown: Any,
    ) -> None:
        given = locals()
        for field in fields(self):
            value = given[field.name]
            if field.name in ("default_headers", "extra_body"):
                # The caller's dicts are copied: a configuration does not
                # change.
                value = copy.deepcopy(value)
            object.__setattr__(self, field.name, value)
        written = json.loads(_native.openai_config_form(self._form()))
        for name in _DECIDED:
            object.__setattr__(self, name, written[name])

    def _form(self) -> str:
        """The configuration's JSON form, as the native reader takes it.

        The key does not cross in it: a key that is text is written masked,
        and handed to a provider apart."""
        form = {field.name: getattr(self, field.name) for field in fields(self)}
        for name in _DECIDED:
            if form[name] is None:
                # A switch left unset is left out, never written null.
                del form[name]
        if isinstance(self.api_key, str):
            form["api_key"] = _MASK if self.api_key else ""
        return _dumps(form)

    def __repr__(self) -> str:
        shown = {field.name: getattr(self, field.name) for field in fields(self)}
        shown["api_key"] = _MASK if self.api_key else ""
        if self.default_headers is not None:
            shown["default_headers"] = dict.fromkeys(self.default_headers, _MASK)
        if isinstance(self.base_url, str) and "@" in self.base_url:
            # What stands before the last `@` may be credentials, whichever
            # way a client reads the URL: only a scheme is shown of it.
            before, _, after = self.base_url.rpartition("@")
            scheme, separator, _ = before.partition("://")
            shown["base_url"] = (
                f"{scheme}{separator}{_MASK}@{after}" if separator else (f"{_MASK}@{after}")
            )
        members = ", ".join(f"{name}={value!r}" for name, value in shown.items())
        return f"{type(self).__name__}({members})"


class OpenAIAIProvider:
    """A provider for OpenAI's Chat Completions, and for the servers that
    speak it behind a base URL.

    A request and what answers it are plain dicts in the contract's JSON
    form (``AIRequest``, ``AIResponse``, the stream events). A failure is a
    ``ProviderError``; a request that does not read is a ``ValueError``.

    Cancelling the task that awaits a call stops its request and closes its
    connection; so does closing a stream, or leaving it unread.
    """

    def __init__(self, config: OpenAIConfig) -> None:
        if not isinstance(config, OpenAIConfig):
            raise TypeError("config must be an OpenAIConfig")
        self._config = config
        self._native = _native._OpenAIProvider(config._form(), config.api_key)

    @property
    def name(self) -> str:
        """The provider's name: its class name."""
        return self.__class__.__name__

    @property
    def model_name(self) -> str:
        """The model the provider asks, as configured."""
        return self._native.model_name

    @property
    def supports_vision(self) -> bool:
        """Whether the configured model takes images."""
        return self._native.supports_vision

    @property
    def supports_streaming(self) -> bool:
        """Whether ``generate_stream`` streams."""
        return self._native.supports_streaming

    @property
    def supports_structured_streaming(self) -> bool:
        """Whether ``generate_structured_stream`` streams."""
        return self._native.supports_structured_streaming

    @property
    def supports_response_schema(self) -> bool:
        """Whether a request's ``response_schema`` is honoured."""
        return self._native.supports_response_schema

    @property
    def supports_response_schema_with_tools(self) -> bool:
        """Whether a response schema may ride a request that carries tools."""
        return self._native.supports_response_schema_with_tools

    @property
    def supports_deferred_tools(self) -> bool:
        """Whether the model can hold a tool declared but unseen."""
        return self._native.supports_deferred_tools

    @classmethod
    def available_models(cls) -> list[ModelInfo]:
        """The curated catalogue of OpenAI's models, with no request."""
        return json.loads(_native._OpenAIProvider.available_models_json())

    async def list_models(self) -> list[ModelInfo]:
        """Every model the endpoint lists, filled from the catalogue where
        it knows the model."""
        return json.loads(await self._native.list_models())

    def catalog_entry(self) -> ModelInfo | None:
        """The catalogue's entry for the configured model, if it has one."""
        entry = self._native.catalog_entry_json()
        return None if entry is None else json.loads(entry)

    @property
    def context_window(self) -> int | None:
        """The configured model's input context window, in tokens, when the
        catalogue knows it."""
        return self._native.context_window

    async def generate(self, request: AIRequest) -> AIResponse:
        """One whole answer to ``request``."""
        return json.loads(await self._native.generate(_dumps(request)))

    async def generate_stream(self, request: AIRequest) -> AsyncIterator[str]:
        """The text of one answer to ``request``, as it comes.

        Nothing is sent before the first item is asked for. The answer is
        then read a bounded number of items ahead of those taken."""
        stream = self._native.stream(_dumps(request), True)
        try:
            while (pieces := await stream.next()) is not None:
                for piece in json.loads(pieces):
                    yield piece
        finally:
            stream.close()

    async def generate_structured_stream(self, request: AIRequest) -> AsyncIterator[StreamEvent]:
        """The events of one answer to ``request``, as they come: its text
        and its reasoning, each call as it is written, then the whole calls
        and the done event.

        Nothing is sent before the first event is asked for. The answer is
        then read a bounded number of events ahead of those taken. A failure
        is raised after the events read before it."""
        stream = self._native.stream(_dumps(request), False)
        try:
            while (events := await stream.next()) is not None:
                for event in json.loads(events):
                    yield event
        finally:
            stream.close()

    async def close(self) -> None:
        """Release what the provider holds."""
        await self._native.close()
