"""A LiteLLM proxy's Chat Completions, in front of the upstreams it routes to.

    from roomkit_providers.llm.litellm import LiteLLM, LiteLLMConfig

    provider = LiteLLM(LiteLLMConfig(api_key=..., model="claude-sonnet"))

``LiteLLM`` is a RoomKit ``AIProvider``: an AI channel takes it as it takes
RoomKit's own ``LiteLLMAIProvider``.
"""

from __future__ import annotations

from typing import Any

from pydantic import SecretStr
from roomkit.providers.litellm import LiteLLMConfig as RoomKitLiteLLMConfig

from ._config import Config
from ._provider import Provider

__all__ = ["LiteLLM", "LiteLLMConfig"]


class LiteLLMConfig(Config, vendor="litellm", stands_for=RoomKitLiteLLMConfig):
    """What a LiteLLM provider is configured with: RoomKit's ``LiteLLMConfig``,
    an ``OpenAIConfig`` with LiteLLM's defaults.

    Attributes:
        api_key: The key, sent as a bearer token. An empty key sends no
            ``Authorization`` header.
        base_url: The proxy's base URL, with or without ``/v1``: the proxy
            serves its API at both.
        model: The public name the proxy routes, the ``model_name`` of its
            configuration. Required.
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
        supports_reasoning_effort_with_tools: Whether the proxy takes a
            reasoning setting beside tools; ``None`` and ``False`` leave it
            out, the model behind the name being unknown, ``None`` warning
            of it once.
        thinking_field: The field a server of one's own reads a round's
            reasoning in; unset, no reasoning is sent back.

    ``repr`` never shows the key, the value of a default header or of a
    member of ``extra_body``, nor more of a base URL than its scheme, its
    host and its port.
    """

    api_key: SecretStr
    base_url: str = "http://localhost:4000"
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


class LiteLLM(Provider[LiteLLMConfig | RoomKitLiteLLMConfig], config=LiteLLMConfig):
    """A model a LiteLLM proxy routes to, as a RoomKit provider: a reasoning
    setting goes as LiteLLM translates it for every upstream, on a turn with
    tools only where the configuration says the proxy takes it, and its
    listing is the proxy's deployments with their prices.

    ``config`` is this module's ``LiteLLMConfig``, or RoomKit's own, for a
    host that moves from RoomKit's ``LiteLLMAIProvider``. It is read once,
    when the provider is built. There is no ``transport`` parameter: the
    provider builds its own HTTP client.

    The environment of the process is read then too, and only then, for
    what RoomKit's provider reads there of the host's network: the proxies
    and the certificate authorities. No key and no URL is read from it.

    Cancelling the task that awaits a call stops its request and closes its
    connection; so does closing a stream, or leaving it unread.
    """
