"""An Azure resource's deployments: OpenAI's Chat Completions at their own
paths, the key in Azure's ``api-key`` header.

    from roomkit_providers.llm.azure import AzureAI, AzureAIConfig

    provider = AzureAI(
        AzureAIConfig(
            api_key=..., azure_endpoint="https://acme.openai.azure.com", model="chat-prod"
        )
    )

``AzureAI`` is a RoomKit ``AIProvider``: an AI channel takes it as it takes
RoomKit's own ``AzureAIProvider``.
"""

from __future__ import annotations

from typing import Any

from pydantic import SecretStr
from roomkit.providers.azure import AzureAIConfig as RoomKitAzureAIConfig

from ._config import Config
from ._provider import Provider

__all__ = ["AzureAI", "AzureAIConfig"]


class AzureAIConfig(
    Config,
    vendor="azure",
    stands_for=RoomKitAzureAIConfig,
    rewritten=("azure_endpoint",),
    urls=("azure_endpoint",),
):
    """What an Azure provider is configured with: RoomKit's ``AzureAIConfig``,
    no ``OpenAIConfig``: an endpoint and an API version in place of a base
    URL, no default headers and no thinking field.

    Attributes:
        api_key: The resource's key, sent as its ``api-key`` header. A call
            with an empty key is refused before any request.
        azure_endpoint: The resource's endpoint, as Azure hands it out: its
            root, the v1 API's URL or a deployment's target URI, reduced to
            the resource's base as RoomKit's configuration reduces it.
        api_version: The API version every request asks.
        model: The deployment to ask, a name its operator chose. Required.
        max_tokens: The most tokens an answer may hold.
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
        extra_body: Members merged into every request's body.
        supports_response_schema: Whether the deployment honours a JSON
            schema response format; ``None`` is the provider's own answer.
        supports_response_schema_with_tools: Whether the deployment takes a
            response schema beside tools; ``None`` is the provider's own
            answer, which sends it.
        supports_reasoning_effort_with_tools: Whether the deployment takes
            a reasoning setting beside tools; ``None`` and ``False`` leave
            it out, a deployment's name saying nothing of its model,
            ``None`` warning of it once.

    ``repr`` never shows the key, the value of a member of ``extra_body``,
    nor more of the endpoint than its scheme, its host and its port.
    """

    api_key: SecretStr
    azure_endpoint: str
    api_version: str = "2024-12-01-preview"
    model: str
    max_tokens: int = 1024
    timeout: float = 30.0
    connect_timeout: float = 5.0
    max_retries: int = 0
    include_stream_usage: bool = True
    use_max_completion_tokens: bool = False
    supports_custom_temperature: bool = True
    reasoning_effort: str | None = None
    extra_body: dict[str, Any] | None = None
    supports_response_schema: bool | None = None
    supports_response_schema_with_tools: bool | None = None
    supports_reasoning_effort_with_tools: bool | None = None


class AzureAI(Provider[AzureAIConfig | RoomKitAzureAIConfig], config=AzureAIConfig):
    """An Azure resource's deployment, as a RoomKit provider: OpenAI's wire at
    the deployment's path, the API version in the query and the key in
    ``api-key``; a reasoning setting on a turn with tools only where the
    configuration says the deployment takes it.

    ``config`` is this module's ``AzureAIConfig``, or RoomKit's own, for a
    host that moves from RoomKit's ``AzureAIProvider``. It is read once,
    when the provider is built. There is no ``transport`` parameter: the
    provider builds its own HTTP client.

    The environment of the process is read then too, and only then, for
    what RoomKit's provider reads there of the host's network: the proxies
    and the certificate authorities. No key and no URL is read from it.

    Cancelling the task that awaits a call stops its request and closes its
    connection; so does closing a stream, or leaving it unread.
    """
