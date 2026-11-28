"""Gemini's image models, on the Interactions API.

    from roomkit_providers.image.gemini import GeminiImage, GeminiImageConfig

    provider = GeminiImage(GeminiImageConfig(api_key=...))

``GeminiImage`` is a RoomKit ``ImageProvider``: it takes the place of
RoomKit's own ``GeminiImageProvider``.
"""

from __future__ import annotations

from pydantic import SecretStr
from roomkit.providers.gemini import GeminiImageConfig as RoomKitGeminiImageConfig

from ._config import Config
from ._provider import Provider

__all__ = ["GeminiImage", "GeminiImageConfig"]


class GeminiImageConfig(Config, vendor="gemini", stands_for=RoomKitGeminiImageConfig):
    """What a Gemini image provider is configured with: RoomKit's
    ``GeminiImageConfig``.

    Attributes:
        api_key: The key, sent in ``x-goog-api-key``.
        model: The image model to ask; ``gemini-nano-banana-2.1`` when left
            out, as RoomKit's.
        image_size: The resolution tier every request asks unless it asks a
            size of its own (``512``, ``1K``, ``2K``, ``4K``).
        output_mime_type: The media type every request asks unless it asks
            its own (``image/jpeg``).
        timeout: The time, in seconds, an interaction may take to answer.
        connect_timeout: The time, in seconds, a connection may take.

    ``repr`` never shows the key.
    """

    api_key: SecretStr
    model: str = "gemini-nano-banana-2.1"
    image_size: str | None = None
    output_mime_type: str | None = None
    timeout: float = 120.0
    connect_timeout: float = 5.0


class GeminiImage(Provider[GeminiImageConfig | RoomKitGeminiImageConfig], config=GeminiImageConfig):
    """Gemini's image models, as a RoomKit image provider: one interaction
    per image, ``n`` made at once and each settled alone, a size asked as
    Gemini's ratio and tier, an interaction cut at the decode limit
    continued as google-genai continues it.

    ``config`` is this module's ``GeminiImageConfig``, or RoomKit's own, for
    a host that moves from RoomKit's ``GeminiImageProvider``. It is read
    once, when the provider is built.
    """
