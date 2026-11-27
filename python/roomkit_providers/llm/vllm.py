"""A vLLM server's models: OpenAI's Chat Completions, a reasoning setting in
the chat template's switches, vLLM's sampling members in the body.

    from roomkit_providers.llm.vllm import VLLM, VLLMConfig

    provider = VLLM(VLLMConfig(model="qwen3-8b"))

``VLLM`` is a RoomKit ``AIProvider``: an AI channel takes it as it takes the
provider RoomKit's own ``create_vllm_provider`` builds.
"""

from __future__ import annotations

from typing import Any

from pydantic import SecretStr
from roomkit.providers.vllm import VLLMConfig as RoomKitVLLMConfig

from ._config import Config
from ._provider import Provider

__all__ = ["VLLM", "VLLMConfig"]


class VLLMConfig(Config, vendor="vllm", stands_for=RoomKitVLLMConfig):
    """What a vLLM provider is configured with: RoomKit's ``VLLMConfig``, no
    ``OpenAIConfig``: a key that defaults to ``none``, headers in place of
    default headers, vLLM's sampling members and its chat template's
    switches.

    Attributes:
        model: The model the server loaded. Required.
        base_url: The server's OpenAI-compatible endpoint.
        api_key: The bearer token, ``vllm serve --api-key``'s; ``none`` by
            default, for a local server that asks for none.
        max_tokens: The most tokens an answer may hold.
        timeout: The time, in seconds, an answer may take: for a stream,
            the wait between two reads of it.
        connect_timeout: The time, in seconds, a connection may take.
        max_retries: How many times a request that failed for a passing
            reason is sent again.
        include_stream_usage: Whether a stream is asked for its token usage.
        headers: Headers sent with every request, for a reverse proxy or
            another ``Authorization`` scheme.
        top_p: Nucleus sampling's cutoff; ``None`` leaves the server's.
        top_k: Top-k sampling's cutoff, a vLLM member.
        min_p: The least probability a token is sampled at, a vLLM member.
        presence_penalty: The penalty on tokens the answer already holds.
        repetition_penalty: vLLM's multiplicative penalty on repetition.
        extra_body: Members merged into every request's body, over the
            sampling members; its ``chat_template_kwargs`` over the
            template's switches.
        enable_thinking: The chat template's thinking switch; ``None``
            leaves the model's. The turn's switch outranks it.
        reasoning_effort: The chat template's reasoning effort; the turn's
            own outranks it.
        supports_response_schema: Whether the server honours a JSON schema
            response format; ``None`` is the provider's own answer.
        supports_response_schema_with_tools: Whether the server takes a
            response schema beside tools; ``None`` is the provider's own
            answer, which sends none there.

    ``repr`` never shows the key, the value of a header or of a member of
    ``extra_body``, nor more of a base URL than its scheme, its host and its
    port.
    """

    model: str
    base_url: str = "http://localhost:8000/v1"
    api_key: SecretStr = SecretStr("none")
    max_tokens: int = 1024
    timeout: float = 30.0
    connect_timeout: float = 5.0
    max_retries: int = 0
    include_stream_usage: bool = True
    headers: dict[str, str] | None = None
    top_p: float | None = None
    top_k: int | None = None
    min_p: float | None = None
    presence_penalty: float | None = None
    repetition_penalty: float | None = None
    extra_body: dict[str, Any] | None = None
    enable_thinking: bool | None = None
    reasoning_effort: str | None = None
    supports_response_schema: bool | None = None
    supports_response_schema_with_tools: bool | None = None


class VLLM(Provider[VLLMConfig | RoomKitVLLMConfig], config=VLLMConfig):
    """A model a vLLM server loaded, as a RoomKit provider: a reasoning
    setting rides the chat template's ``chat_template_kwargs``, the turn's
    over the configuration's, on a turn with tools as on any other; vLLM's
    sampling members ride the body.

    ``config`` is this module's ``VLLMConfig``, or RoomKit's own, for a host
    that moves from RoomKit's ``create_vllm_provider``. It is read once, when
    the provider is built. There is no ``transport`` parameter: the provider
    builds its own HTTP client.

    The environment of the process is read then too, and only then, for
    what RoomKit's provider reads there of the host's network: the proxies
    and the certificate authorities. No key and no URL is read from it.

    Cancelling the task that awaits a call stops its request and closes its
    connection; so does closing a stream, or leaving it unread.
    """
