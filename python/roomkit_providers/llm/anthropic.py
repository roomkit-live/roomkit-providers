"""Anthropic's Messages API, and the servers that speak it behind a base URL.

    from roomkit_providers.llm.anthropic import Anthropic, AnthropicConfig

    provider = Anthropic(AnthropicConfig(api_key=..., model="claude-sonnet-4-6"))

``Anthropic`` is a RoomKit ``AIProvider``: an AI channel takes it as it takes
RoomKit's own Anthropic provider.
"""

from __future__ import annotations

from pydantic import SecretStr, field_validator
from roomkit.providers.anthropic import AnthropicConfig as RoomKitAnthropicConfig

from ._config import Config
from ._provider import Provider

__all__ = ["Anthropic", "AnthropicConfig"]


class AnthropicConfig(
    Config,
    vendor="anthropic",
    stands_for=RoomKitAnthropicConfig,
    # The switches a modern model decides on Anthropic's own endpoint,
    # unless stated.
    decided=("use_adaptive_thinking", "supports_custom_temperature"),
):
    """What an Anthropic provider is configured with.

    Attributes:
        api_key: The key, sent in ``x-api-key``.
        model: The model to ask. Required, so that an upgrade never changes
            a caller's model.
        max_tokens: The most tokens an answer may hold.
        timeout: The time, in seconds, an answer may take: for a stream,
            the wait between two reads of it.
        connect_timeout: The time, in seconds, a connection may take.
        max_retries: How many times a request that failed for a passing
            reason is sent again.
        base_url: The endpoint of a server that speaks the Messages API.
            ``None`` is Anthropic's own. A URL that ends with
            ``/v1/messages`` is kept without it.
        extra_headers: Headers sent with every request.
        enable_prompt_caching: Whether the stable start of a request is
            marked for the prompt cache. Set it false for a server that
            refuses ``cache_control``.
        use_adaptive_thinking: Whether thinking is asked as adaptive rather
            than with a budget. Left unset, a modern model on Anthropic's
            own endpoint decides it.
        supports_custom_temperature: Whether a temperature is sent. Left
            unset, a modern model on Anthropic's own endpoint decides it.

    ``repr`` never shows the key, the value of an extra header, nor more of
    a base URL than its scheme, its host and its port.
    """

    api_key: SecretStr
    model: str
    max_tokens: int = 1024
    timeout: float = 60.0
    connect_timeout: float = 5.0
    max_retries: int = 0
    base_url: str | None = None
    extra_headers: dict[str, str] | None = None
    enable_prompt_caching: bool = True
    use_adaptive_thinking: bool = False
    supports_custom_temperature: bool = True

    @field_validator("base_url")
    @classmethod
    def _without_the_path_of_the_api(cls, value: str | None) -> str | None:
        """A pasted endpoint as the base it stands for: without the white
        space around it, the slashes that end it and the path of the
        Messages API, as RoomKit's configuration keeps it."""
        if value is None:
            return value
        trimmed = value.strip().rstrip("/")
        path = "/v1/messages"
        if trimmed.lower().endswith(path):
            trimmed = trimmed[: -len(path)]
        return trimmed or value.strip()


class Anthropic(Provider[AnthropicConfig | RoomKitAnthropicConfig], config=AnthropicConfig):
    """Anthropic's Messages API, and the servers that speak it behind a base
    URL, as a RoomKit provider.

    ``config`` is this module's ``AnthropicConfig``, or RoomKit's own, for a
    host that moves from RoomKit's Anthropic provider. It is read once, when
    the provider is built. The provider builds its own HTTP client.

    The environment of the process is read then too, and only then, for
    what RoomKit's provider reads there of the host's network: the proxies
    (``HTTP_PROXY``, ``HTTPS_PROXY``, ``ALL_PROXY``, ``NO_PROXY``) and the
    certificate authorities (``SSL_CERT_FILE``, ``SSL_CERT_DIR``, else the
    system's). No key, no URL and no header is read from it.

    A context may lend one request a key of its own
    (``metadata["api_key"]``): it is sent in place of the configuration's,
    for that request alone.

    Cancelling the task that awaits a call stops its request and closes its
    connection; so does closing a stream, or leaving it unread.
    """
