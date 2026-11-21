"""OpenAI's Chat Completions, and the servers that speak it behind a base URL.

    from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

    provider = OpenAI(OpenAIConfig(api_key=..., model="gpt-4o"))

``OpenAI`` is a RoomKit ``AIProvider``: an AI channel takes it as it takes
RoomKit's own OpenAI provider.
"""

from __future__ import annotations

import json
from collections.abc import Iterator
from typing import Any
from urllib.parse import urlsplit

from pydantic import BaseModel, SecretStr
from roomkit.providers.openai import OpenAIConfig as RoomKitOpenAIConfig

from .. import _native, _process
from ._provider import Provider, failing_as_roomkit

__all__ = ["OpenAI", "OpenAIConfig"]

_MASK = "**********"
# The switches a model decides on OpenAI's own endpoint, unless stated.
_PROFILED = {"use_max_completion_tokens", "supports_custom_temperature"}


def _profile(model: str, base_url: str | None) -> dict[str, bool]:
    """What `model` behind `base_url` decides of the profiled switches."""
    _process.enter()
    asked = {"api_key": "", "model": model, "base_url": base_url}
    decided = json.loads(_native.openai_config_form(json.dumps(asked)))
    return {name: decided[name] for name in _PROFILED}


def _catalogue() -> str:
    """OpenAI's curated catalogue, as a JSON list."""
    _process.enter()
    return _native.openai_available_models_json()


def _shown_url(url: str) -> str:
    """`url` as it is shown: its scheme, its host and its port. A user and a
    password, a path and a query may each hold a credential, and a URL that
    does not read is not shown at all."""
    try:
        read = urlsplit(url)
        host, port = read.hostname, read.port
    except ValueError:
        return _MASK
    if not read.scheme or host is None:
        return _MASK
    host = f"[{host}]" if ":" in host else host
    return f"{read.scheme}://{host}" + ("" if port is None else f":{port}")


class _Veiled:
    """A configuration held so that no frame shows it: what a traceback
    writes of a frame's locals is each one's ``repr``, and a configuration's
    may show a header's value or a URL's password.

    Nothing here binds a secret to a name where something can still raise:
    the key leaves as the object that masks it, which the native module
    opens, and the JSON form is handed on as it is written.
    """

    __slots__ = ("_held",)

    def __init__(self, held: Any) -> None:
        self._held = held

    def __repr__(self) -> str:
        return "<a configuration>"

    def kind(self) -> type:
        return type(self._held)

    def held(self) -> Any:
        return self._held

    def own(self) -> OpenAIConfig:
        """The configuration as this module's, whose ``repr`` shows no
        secret: RoomKit's own is copied member by member, unchecked, since
        RoomKit checked it."""
        if type(self._held) is OpenAIConfig:
            return self._held
        return OpenAIConfig.model_construct(
            _fields_set=set(OpenAIConfig.model_fields),
            **{name: getattr(self._held, name) for name in OpenAIConfig.model_fields},
        )

    def native(self) -> _native._Provider:
        """The native provider of the configuration."""
        own = self.own()
        return _native.openai(own.model_dump_json(), own.api_key)


class OpenAIConfig(BaseModel):
    """What an OpenAI provider is configured with.

    Attributes:
        api_key: The key, sent as a bearer token. An empty key sends no
            ``Authorization`` header.
        base_url: The endpoint of an OpenAI-compatible server. ``None`` is
            OpenAI's own.
        model: The model to ask. Required, so that an upgrade never changes
            a caller's model.
        max_tokens: The most tokens an answer may hold; ``None`` sends no
            bound, and the endpoint decides.
        timeout: The time, in seconds, an answer may take: for a stream,
            the wait between two reads of it.
        connect_timeout: The time, in seconds, a connection may take.
        max_retries: How many times a request that failed for a passing
            reason is sent again.
        include_stream_usage: Whether a stream is asked for its token
            usage. Set it false for a server that refuses
            ``stream_options``.
        use_max_completion_tokens: Whether the token bound is sent as
            ``max_completion_tokens`` rather than ``max_tokens``. Left
            unset, a modern model on OpenAI's own endpoint decides it.
        supports_custom_temperature: Whether a temperature is sent. Left
            unset, a reasoning model on OpenAI's own endpoint decides it.
        reasoning_effort: The reasoning depth of a reasoning model;
            ``None`` is the model's own.
        default_headers: Headers sent with every request.
        extra_body: Members merged into every request's body, for what a
            server takes beyond OpenAI's schema.
        supports_response_schema: Whether the server honours a JSON schema
            response format; ``None`` is the provider's own answer.
        supports_response_schema_with_tools: Whether the server takes a
            response schema beside tools; ``None`` is the provider's own
            answer.
        supports_reasoning_effort_with_tools: Whether the server takes a
            reasoning effort beside tools, for a model the provider cannot
            know.
        thinking_field: The assistant message member this server reads a
            model's earlier reasoning from; ``None`` sends none back.

    ``repr`` never shows the key, the value of a default header or of a
    member of ``extra_body``, nor more of a base URL than its scheme, its
    host and its port.
    """

    api_key: SecretStr
    base_url: str | None = None
    model: str
    max_tokens: int | None = 1024
    timeout: float = 30.0
    connect_timeout: float = 5.0
    max_retries: int = 0
    include_stream_usage: bool = True
    use_max_completion_tokens: bool = False
    supports_custom_temperature: bool = True
    reasoning_effort: str | None = None
    default_headers: dict[str, str] | None = None
    extra_body: dict[str, Any] | None = None
    supports_response_schema: bool | None = None
    supports_response_schema_with_tools: bool | None = None
    supports_reasoning_effort_with_tools: bool | None = None
    thinking_field: str | None = None

    def model_post_init(self, context: Any, /) -> None:
        """Decide the switches a modern model decides on OpenAI's own
        endpoint, each unless it was stated."""
        unset = _PROFILED - self.model_fields_set
        if not unset:
            return
        profile = _profile(self.model, self.base_url)
        for name in sorted(unset):
            if getattr(self, name) != profile[name]:
                setattr(self, name, profile[name])

    def __repr_args__(self) -> Iterator[tuple[str | None, Any]]:
        for name, value in super().__repr_args__():
            if name in ("default_headers", "extra_body") and value is not None:
                value = dict.fromkeys(value, _MASK)
            elif name == "base_url" and isinstance(value, str):
                value = _shown_url(value)
            yield name, value


class OpenAI(Provider):
    """OpenAI's Chat Completions, and the servers that speak it behind a
    base URL, as a RoomKit provider.

    ``config`` is this module's ``OpenAIConfig``, or RoomKit's own, for a
    host that moves from RoomKit's OpenAI provider. It is read once, when
    the provider is built. There is no ``transport`` parameter: the provider
    builds its own HTTP client.

    The environment of the process is read then too, and only then, for
    what RoomKit's provider reads there of the host's network: the proxies
    (``HTTP_PROXY``, ``HTTPS_PROXY``, ``ALL_PROXY``, ``NO_PROXY``) and the
    certificate authorities (``SSL_CERT_FILE``, ``SSL_CERT_DIR``, else the
    system's). No key and no URL is read from it.

    Cancelling the task that awaits a call stops its request and closes its
    connection; so does closing a stream, or leaving it unread.
    """

    _catalogue = _catalogue()

    def __init__(self, config: OpenAIConfig | RoomKitOpenAIConfig) -> None:
        # From here on no frame holds the configuration as it shows itself.
        config = _Veiled(config)
        if config.kind() not in (OpenAIConfig, RoomKitOpenAIConfig):
            raise TypeError(
                "config must be an OpenAIConfig: the configuration of a provider derived "
                "from OpenAI's belongs to that provider"
            )
        _process.enter()
        with failing_as_roomkit():
            native = config.native()
        self._config = config.held()
        super().__init__(native)
