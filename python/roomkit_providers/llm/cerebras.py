"""Cerebras's Chat Completions, a round's reasoning sent back to it.

    from roomkit_providers.llm.cerebras import Cerebras, CerebrasConfig

    provider = Cerebras(CerebrasConfig(api_key=..., model="gpt-oss-120b"))

``Cerebras`` is a RoomKit ``AIProvider``: an AI channel takes it as it takes
RoomKit's own ``CerebrasAIProvider``.
"""

from __future__ import annotations

from typing import Any, Literal

from pydantic import SecretStr, field_validator
from roomkit.providers.cerebras import CerebrasConfig as RoomKitCerebrasConfig

from ._config import Config
from ._provider import Provider

__all__ = ["Cerebras", "CerebrasConfig"]


class CerebrasConfig(Config, vendor="cerebras", stands_for=RoomKitCerebrasConfig):
    """What a Cerebras provider is configured with: RoomKit's ``CerebrasConfig``,
    an ``OpenAIConfig`` with Cerebras's defaults.

    Attributes:
        api_key: The key, sent as a bearer token. An empty key sends no
            ``Authorization`` header.
        base_url: Cerebras's endpoint; another URL points at a proxy or a
            server of one's own.
        model: The model to ask. Required, so that an upgrade never changes
            a caller's model.
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
            applies Cerebras's own rule on a turn with tools.
        thinking_field: Left unset: the provider sends reasoning where
            Cerebras reads it.
        reasoning_format: How the answer carries the model's reasoning:
            ``parsed``, the default, apart from the text; ``raw``, in it;
            ``hidden``, not at all; ``none``, as the model decides.
            ``None`` sends no ``reasoning_format``.
        clear_thinking: Whether the server drops earlier reasoning before
            it prompts the model; ``None`` leaves the server's default.

    ``repr`` never shows the key, the value of a default header or of a
    member of ``extra_body``, nor more of a base URL than its scheme, its
    host and its port.
    """

    api_key: SecretStr
    base_url: str = "https://api.cerebras.ai/v1"
    model: str
    max_tokens: int | None = 1024
    timeout: float = 30.0
    connect_timeout: float = 5.0
    max_retries: int = 0
    include_stream_usage: bool = False
    use_max_completion_tokens: bool = True
    supports_custom_temperature: bool = True
    reasoning_effort: str | None = None
    default_headers: dict[str, str] | None = None
    extra_body: dict[str, Any] | None = None
    supports_response_schema: bool | None = None
    supports_response_schema_with_tools: bool | None = None
    supports_reasoning_effort_with_tools: bool | None = None
    thinking_field: str | None = None
    reasoning_format: Literal["parsed", "raw", "hidden", "none"] | None = "parsed"
    clear_thinking: bool | None = None

    @field_validator("supports_reasoning_effort_with_tools")
    @classmethod
    def _no_tool_turn_setting(cls, value: bool | None) -> bool | None:
        if value is not None:
            raise ValueError(
                f"{cls.__name__} takes no supports_reasoning_effort_with_tools: its "
                "provider applies its vendor's own rule on a turn with tools"
            )
        return value

    @field_validator("thinking_field")
    @classmethod
    def _no_thinking_field(cls, value: str | None) -> str | None:
        if value is not None:
            raise ValueError(
                f"{cls.__name__} takes no thinking_field: its provider sends reasoning "
                "where its vendor reads it"
            )
        return value


class Cerebras(Provider[CerebrasConfig | RoomKitCerebrasConfig], config=CerebrasConfig):
    """A Cerebras model, as a RoomKit provider: a round's reasoning goes back
    in ``reasoning``, its effort floored by the model's catalogue entry on
    a turn with tools too, and an array the model wrote as the JSON text
    of the array is read back where the tool declares one.

    ``config`` is this module's ``CerebrasConfig``, or RoomKit's own, for a
    host that moves from RoomKit's ``CerebrasAIProvider``. It is read once,
    when the provider is built. There is no ``transport`` parameter: the
    provider builds its own HTTP client.

    The environment of the process is read then too, and only then, for
    what RoomKit's provider reads there of the host's network: the proxies
    and the certificate authorities. No key and no URL is read from it.

    Cancelling the task that awaits a call stops its request and closes its
    connection; so does closing a stream, or leaving it unread.
    """
