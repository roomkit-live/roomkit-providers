"""OpenAI's image models: generations and edits, whole or as a stream of
previews.

    from roomkit_providers.image.openai import OpenAIImage, OpenAIImageConfig

    provider = OpenAIImage(OpenAIImageConfig(api_key=..., model="gpt-image-2"))

``OpenAIImage`` is a RoomKit ``ImageProvider``: it takes the place of
RoomKit's own ``OpenAIImageProvider``.
"""

from __future__ import annotations

from pydantic import SecretStr
from roomkit.providers.openai import OpenAIImageConfig as RoomKitOpenAIImageConfig

from ._config import Config
from ._provider import Provider

__all__ = ["OpenAIImage", "OpenAIImageConfig"]


class OpenAIImageConfig(Config, vendor="openai", stands_for=RoomKitOpenAIImageConfig):
    """What an OpenAI image provider is configured with: RoomKit's
    ``OpenAIImageConfig``.

    Attributes:
        api_key: The key, sent as a bearer token.
        base_url: The endpoint, ``https://api.openai.com/v1`` when left out.
        model: The image model to ask (``gpt-image-2``).
        quality: The rendering effort every request asks unless it asks its
            own; ``None`` leaves the model's.
        background: The background every request asks unless it asks its
            own.
        output_format: The encoding every request asks unless it asks its
            own.
        timeout: The time, in seconds, an answer may take.
        connect_timeout: The time, in seconds, a connection may take.
        default_headers: Headers every request carries, which replace the
            provider's of the same name.

    ``repr`` never shows the key, nor a header's value.
    """

    api_key: SecretStr
    base_url: str | None = None
    model: str
    quality: str | None = None
    background: str | None = None
    output_format: str | None = None
    timeout: float = 120.0
    connect_timeout: float = 5.0
    default_headers: dict[str, str] | None = None


class OpenAIImage(Provider[OpenAIImageConfig | RoomKitOpenAIImageConfig], config=OpenAIImageConfig):
    """OpenAI's image models, as a RoomKit image provider: a request with
    reference images is an edit; ``partial_images`` streams previews, each
    told to the progress callback.

    ``config`` is this module's ``OpenAIImageConfig``, or RoomKit's own, for a
    host that moves from RoomKit's ``OpenAIImageProvider``. It is read once,
    when the provider is built. A paid generation is never tried again on
    its own.
    """
