"""OpenAI's Chat Completions, and the servers that speak it behind a base URL.

    from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

    provider = OpenAI(OpenAIConfig(api_key=..., model="gpt-4o"))

``OpenAI`` is a RoomKit ``AIProvider``: an AI channel takes it as it takes
RoomKit's own OpenAI provider.
"""

from __future__ import annotations

from typing import Any

from pydantic import SecretStr
from roomkit.providers.openai import OpenAIConfig as RoomKitOpenAIConfig

from ._config import Config
from ._provider import Provider

__all__ = ["OpenAI", "OpenAIConfig"]


class OpenAIConfig(
    Config,
    vendor="openai",
    stands_for=RoomKitOpenAIConfig,
    # The switches a model decides on OpenAI's own endpoint, unless stated.
    decided=("use_max_completion_tokens", "supports_custom_temperature"),
):
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


class OpenAI(Provider[OpenAIConfig | RoomKitOpenAIConfig], config=OpenAIConfig):
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
