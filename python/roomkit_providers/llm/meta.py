"""Meta's Chat Completions: Muse, whose every chat model reasons.

    from roomkit_providers.llm.meta import Meta, MetaConfig

    provider = Meta(MetaConfig(api_key=...))

``Meta`` is a RoomKit ``AIProvider``: an AI channel takes it as it takes
RoomKit's own ``MetaAIProvider``.
"""

from __future__ import annotations

from typing import Any

from pydantic import SecretStr, field_validator
from roomkit.providers.meta import MetaConfig as RoomKitMetaConfig

from ._config import Config
from ._provider import Provider

__all__ = ["Meta", "MetaConfig"]


class MetaConfig(Config, vendor="meta", stands_for=RoomKitMetaConfig):
    """What a Meta provider is configured with: RoomKit's ``MetaConfig``,
    an ``OpenAIConfig`` with Meta's defaults.

    Attributes:
        api_key: The key, sent as a bearer token. An empty key sends no
            ``Authorization`` header; one that is not ASCII, which no
            header can carry, is refused, as RoomKit's configuration
            refuses it.
        base_url: Meta's endpoint; another URL points at a proxy or a
            server of one's own.
        model: The model to ask; ``muse-spark-1.3`` when left out, as
            RoomKit's.
        max_tokens: The most tokens an answer may hold; ``None`` sends no
            bound.
        timeout: The time, in seconds, an answer may take: for a stream,
            the wait between two reads of it.
        connect_timeout: The time, in seconds, a connection may take.
        max_retries: How many times a request that failed for a passing
            reason is sent again.
        include_stream_usage: Whether a stream is asked for its token usage.
        use_max_completion_tokens: Whether the token bound is sent as
            ``max_completion_tokens`` rather than ``max_tokens``.
        supports_custom_temperature: Whether a temperature is sent.
        reasoning_effort: The reasoning depth; the turn's own outranks it.
        default_headers: Headers sent with every request.
        extra_body: Members merged into every request's body.
        supports_response_schema: Whether the server honours a JSON schema
            response format; ``None`` is the provider's own answer.
        supports_response_schema_with_tools: Whether the server takes a
            response schema beside tools; ``None`` is the provider's own
            answer.
        supports_reasoning_effort_with_tools: Left unset: the provider
            applies Meta's own rule on a turn with tools.
        thinking_field: The field a server of one's own reads a round's reasoning
            in; unset, no reasoning is sent back.

    ``repr`` never shows the key, the value of a default header or of a
    member of ``extra_body``, nor more of a base URL than its scheme, its
    host and its port.
    """

    api_key: SecretStr
    base_url: str = "https://api.meta.ai/v1"
    model: str = "muse-spark-1.3"
    max_tokens: int | None = 1024
    timeout: float = 30.0
    connect_timeout: float = 5.0
    max_retries: int = 0
    include_stream_usage: bool = True
    use_max_completion_tokens: bool = True
    supports_custom_temperature: bool = True
    reasoning_effort: str | None = None
    default_headers: dict[str, str] | None = None
    extra_body: dict[str, Any] | None = None
    supports_response_schema: bool | None = None
    supports_response_schema_with_tools: bool | None = None
    supports_reasoning_effort_with_tools: bool | None = None
    thinking_field: str | None = None

    @field_validator("api_key")
    @classmethod
    def _header_safe(cls, value: SecretStr) -> SecretStr:
        # The words are RoomKit's, and never the key's.
        if not value.get_secret_value().isascii():
            raise ValueError(
                "api_key holds a non-ASCII character (a placeholder such as '\u2026' "
                "copied as is?): an HTTP Authorization header cannot carry it"
            )
        return value

    @field_validator("supports_reasoning_effort_with_tools")
    @classmethod
    def _no_tool_turn_setting(cls, value: bool | None) -> bool | None:
        if value is not None:
            raise ValueError(
                f"{cls.__name__} takes no supports_reasoning_effort_with_tools: its "
                "provider applies its vendor's own rule on a turn with tools"
            )
        return value


class Meta(Provider[MetaConfig | RoomKitMetaConfig], config=MetaConfig):
    """A Muse model of Meta, as a RoomKit provider: its effort rides every
    request, tool turns included, a turn that states off sent ``minimal``,
    the floor of every Muse chat model; its listing keeps the chat models.

    ``config`` is this module's ``MetaConfig``, or RoomKit's own, for a
    host that moves from RoomKit's ``MetaAIProvider``. It is read once,
    when the provider is built. There is no ``transport`` parameter: the
    provider builds its own HTTP client.

    The environment of the process is read then too, and only then, for
    what RoomKit's provider reads there of the host's network: the proxies
    and the certificate authorities. No key and no URL is read from it.

    Cancelling the task that awaits a call stops its request and closes its
    connection; so does closing a stream, or leaving it unread.
    """
