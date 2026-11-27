"""Mistral's models, on its chat completions.

    from roomkit_providers.llm.mistral import Mistral, MistralConfig

    provider = Mistral(MistralConfig(api_key=...))

``Mistral`` is a RoomKit ``AIProvider``: an AI channel takes it as it takes
RoomKit's own ``MistralAIProvider``.
"""

from __future__ import annotations

from pydantic import SecretStr
from roomkit.providers.mistral import MistralConfig as RoomKitMistralConfig

from ._config import Config
from ._provider import Provider

__all__ = ["Mistral", "MistralConfig"]


class MistralConfig(
    Config, vendor="mistral", stands_for=RoomKitMistralConfig, urls=("server_url",)
):
    """What a Mistral provider is configured with: RoomKit's ``MistralConfig``.

    Attributes:
        api_key: The key, sent as a bearer token.
        model: The model to ask; ``mistral-large-latest`` when left out, as
            RoomKit's.
        max_tokens: The most tokens an answer may hold, which a request's
            own outranks.
        server_url: The base URL of a server that speaks Mistral's API;
            ``None`` asks Mistral's own endpoint.
        reasoning_effort: The reasoning effort of a model that takes one
            (``high``, ``none``), which a request's outranks; ``None`` leaves
            the model's.

    ``repr`` never shows the key, nor more of ``server_url`` than its
    scheme, its host and its port.
    """

    api_key: SecretStr
    model: str = "mistral-large-latest"
    max_tokens: int = 1024
    server_url: str | None = None
    reasoning_effort: str | None = None


class Mistral(Provider[MistralConfig | RoomKitMistralConfig], config=MistralConfig):
    """A Mistral model, as a RoomKit provider: every generation asked as a
    stream, a turn's reasoning as Mistral's ``reasoning_effort``, read back
    from ``<think>`` tags or from typed parts of reasoning, a tool's name
    checked on Mistral's own endpoint.

    ``config`` is this module's ``MistralConfig``, or RoomKit's own, for a
    host that moves from RoomKit's ``MistralAIProvider``. It is read once,
    when the provider is built. There is no ``transport`` parameter: the
    provider builds its own HTTP client.

    The environment of the process is read then too, and only then, for
    what RoomKit's provider reads there of the host's network: the proxies
    and the certificate authorities. No key and no URL is read from it.

    Cancelling the task that awaits a call stops its request and closes its
    connection; so does closing a stream, or leaving it unread.
    """
