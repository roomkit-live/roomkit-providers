"""Google's Gemini models, on the Developer API.

    from roomkit_providers.llm.gemini import Gemini, GeminiConfig

    provider = Gemini(GeminiConfig(api_key=...))

``Gemini`` is a RoomKit ``AIProvider``: an AI channel takes it as it takes
RoomKit's own ``GeminiAIProvider``.
"""

from __future__ import annotations

from pydantic import SecretStr
from roomkit.providers.gemini import GeminiConfig as RoomKitGeminiConfig

from ._config import Config
from ._provider import Provider

__all__ = ["Gemini", "GeminiConfig"]


class GeminiConfig(Config, vendor="gemini", stands_for=RoomKitGeminiConfig):
    """What a Gemini provider is configured with: RoomKit's ``GeminiConfig``.

    Attributes:
        api_key: The key, sent in ``x-goog-api-key``.
        model: The model to ask; ``gemini-3.8-flash`` when left out, as
            RoomKit's.
        max_tokens: The most tokens an answer may hold, which a request's
            own outranks.
        thinking_level: The thinking level of a Gemini 3 model (``minimal``,
            ``low``, ``medium``, ``high``), which a request's effort
            outranks; ``None`` leaves the model's.
        timeout: The time, in seconds, the first read of an answer, and each
            one after it, may take.
        connect_timeout: The time, in seconds, a connection may take.

    ``repr`` never shows the key.
    """

    api_key: SecretStr
    model: str = "gemini-3.8-flash"
    max_tokens: int = 1024
    thinking_level: str | None = None
    timeout: float = 60.0
    connect_timeout: float = 5.0


class Gemini(Provider[GeminiConfig | RoomKitGeminiConfig], config=GeminiConfig):
    """A Gemini model, as a RoomKit provider: every generation asked as a
    stream, a turn's reasoning as Gemini's thinking config, a round's calls
    sent back with their thought signatures, a tool's schema as Gemini
    declares it.

    ``config`` is this module's ``GeminiConfig``, or RoomKit's own, for a
    host that moves from RoomKit's ``GeminiAIProvider``. It is read once,
    when the provider is built. There is no ``transport`` parameter: the
    provider builds its own HTTP client.

    The environment of the process is read then too, and only then, for
    what RoomKit's provider reads there of the host's network: the proxies
    and the certificate authorities. No key and no URL is read from it.

    Cancelling the task that awaits a call stops its request and closes its
    connection; so does closing a stream, or leaving it unread.
    """
