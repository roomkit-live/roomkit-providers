"""The models of a PolarGrid edge, on its chat completions.

    from roomkit_providers.llm.polargrid import PolarGrid, PolarGridConfig

    provider = PolarGrid(PolarGridConfig(api_key=..., region="montreal"))

``PolarGrid`` is a RoomKit ``AIProvider``: an AI channel takes it as it takes
RoomKit's own ``PolarGridAIProvider``.
"""

from __future__ import annotations

from pydantic import Field, SecretStr
from roomkit.providers.polargrid import PolarGridConfig as RoomKitPolarGridConfig

from ._config import Config
from ._provider import Provider

__all__ = ["PolarGrid", "PolarGridConfig"]


class PolarGridConfig(
    Config, vendor="polargrid", stands_for=RoomKitPolarGridConfig, rewritten=("region",)
):
    """What a PolarGrid provider is configured with: RoomKit's
    ``PolarGridConfig``.

    Attributes:
        api_key: The key, sent as a bearer token; an empty one sends the key
            ``POLARGRID_API_KEY`` holds, as polargrid-sdk reads it.
        model: The model to ask; ``qwen-3.8-27b`` when left out, as
            RoomKit's.
        region: The edge to pin: its id (``yul-01``) or its name
            (``montreal``), in any case; an edge PolarGrid does not run is
            refused. ``None`` asks the autorouter, at the first call, for the
            nearest edge that serves the model.
        max_tokens: The most tokens an answer may hold, at least 1 and at
            most 4096 sent, which a request's own outranks; ``None`` sends
            what the model's window leaves after the prompt, at most 4096.
        top_p: The nucleus sampling probability, from 0 to 1.
        thinking: Whether the model reasons (``enable_thinking``), which a
            request's own outranks; ``None`` leaves the model's.
        timeout: The time, in seconds, an answer may take.
        connect_timeout: The time, in seconds, a connection may take.
        max_retries: How many times a request that got no answer is sent
            again; a stream is sent once.
        debug: RoomKit's member, which nothing reads here: the SDK's logging.

    ``repr`` never shows the key.
    """

    api_key: SecretStr
    model: str = "qwen-3.8-27b"
    region: str | None = None
    max_tokens: int | None = Field(default=None, ge=1)
    top_p: float = 0.9
    thinking: bool | None = None
    timeout: float = 30.0
    connect_timeout: float = 5.0
    max_retries: int = 0
    debug: bool = False


class PolarGrid(Provider[PolarGridConfig | RoomKitPolarGridConfig], config=PolarGridConfig):
    """A model of a PolarGrid edge, as a RoomKit provider: the edge pinned or
    picked by the autorouter, a turn's thinking switch as
    ``enable_thinking``, read back from ``<think>`` tags, an answer bounded
    by what the model's window leaves.

    ``config`` is this module's ``PolarGridConfig``, or RoomKit's own, for a
    host that moves from RoomKit's ``PolarGridAIProvider``. It is read once,
    when the provider is built. There is no ``transport`` parameter: the
    provider builds its own HTTP client.

    The environment of the process is read then too, and only then, for
    what RoomKit's provider reads there of the host's network: the proxies
    and the certificate authorities; and, for an empty key,
    ``POLARGRID_API_KEY``.

    RoomKit's ``available_regions`` and ``connected_region`` are not here
    yet: the Rust provider has them, and this module gives a provider's
    contract only.

    Cancelling the task that awaits a call stops its request and closes its
    connection; so does closing a stream, or leaving it unread.
    """
