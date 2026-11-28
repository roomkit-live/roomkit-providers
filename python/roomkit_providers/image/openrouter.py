"""OpenRouter's image models, on its Image API: the image models it routes to,
each request answered by one image.

    from roomkit_providers.image.openrouter import OpenRouterImage, OpenRouterImageConfig

    provider = OpenRouterImage(OpenRouterImageConfig(api_key=..., model="openai/gpt-image-2"))

``OpenRouterImage`` is a RoomKit ``ImageProvider``: it takes the place of
RoomKit's own ``OpenRouterImageProvider``.
"""

from __future__ import annotations

from pydantic import SecretStr
from roomkit.providers.openrouter import OpenRouterImageConfig as RoomKitOpenRouterImageConfig

from ._config import Config
from ._provider import Provider

__all__ = ["OpenRouterImage", "OpenRouterImageConfig"]


class OpenRouterImageConfig(
    Config, vendor="openrouter", stands_for=RoomKitOpenRouterImageConfig, urls=("site_url",)
):
    """What an OpenRouter image provider is configured with: RoomKit's
    ``OpenRouterImageConfig``.

    Attributes:
        api_key: The key, sent as a bearer token.
        base_url: OpenRouter's endpoint, ``https://openrouter.ai/api/v1`` when
            left out.
        model: The routed image model (``openai/gpt-image-2``).
        quality: The rendering effort every request asks.
        background: The background every request asks.
        output_format: The encoding every request asks.
        timeout: The time, in seconds, an answer may take.
        connect_timeout: The time, in seconds, a connection may take.
        default_headers: Headers every request carries, which replace the
            provider's of the same name.
        site_url: The site sent as ``HTTP-Referer``, OpenRouter's attribution.
        app_name: The application named in ``X-Title``, OpenRouter's
            attribution.

    ``repr`` never shows the key, nor a header's value.
    """

    api_key: SecretStr
    base_url: str = "https://openrouter.ai/api/v1"
    model: str
    quality: str | None = None
    background: str | None = None
    output_format: str | None = None
    timeout: float = 120.0
    connect_timeout: float = 5.0
    default_headers: dict[str, str] | None = None
    site_url: str | None = None
    app_name: str | None = None


class OpenRouterImage(
    Provider[OpenRouterImageConfig | RoomKitOpenRouterImageConfig], config=OpenRouterImageConfig
):
    """OpenRouter's image models, as a RoomKit image provider: ``n`` images
    are ``n`` requests made at once, at most ten, the first failure stopping
    the others; reference images ride the same request.

    ``config`` is this module's ``OpenRouterImageConfig``, or RoomKit's own,
    for a host that moves from RoomKit's ``OpenRouterImageProvider``. It is
    read once, when the provider is built. A paid generation is never tried
    again on its own.
    """
