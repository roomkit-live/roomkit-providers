"""The models an Ollama server holds, on its native API.

    from roomkit_providers.llm.ollama import Ollama, OllamaConfig

    provider = Ollama(OllamaConfig(model="qwen3:8b"))

``Ollama`` is a RoomKit ``AIProvider``: an AI channel takes it as it takes
RoomKit's own ``OllamaAIProvider``.
"""

from __future__ import annotations

from typing import Literal

from pydantic import SecretStr, field_validator
from roomkit.providers.ollama import OllamaConfig as RoomKitOllamaConfig

from ._config import Config
from ._provider import Provider

__all__ = ["Ollama", "OllamaConfig"]


class OllamaConfig(Config, vendor="ollama", stands_for=RoomKitOllamaConfig, urls=("host",)):
    """What an Ollama provider is configured with: RoomKit's ``OllamaConfig``.

    Attributes:
        host: The server, a URL or a host and a port, as the ollama SDK
            reads it; the local daemon when left out.
        model: The model to ask; ``llama3.2`` when left out, as RoomKit's.
        max_tokens: The most tokens an answer may hold (``num_predict``),
            which a request's own outranks; ``None`` leaves the server's.
        timeout: The time, in seconds, a read or a write may take.
        connect_timeout: The time, in seconds, a connection may take.
        max_retries: RoomKit's member, which nothing reads: no request is
            tried again.
        think: Whether the model reasons (``True``, ``False``), or how hard
            (``low``, ``medium``, ``high``: RoomKit's ``ThinkEffort``), on a
            model that takes a level; ``None`` leaves the model's.
        keep_alive: How long the model stays loaded: a duration (``"5m"``)
            or a number of seconds, a text of a whole number read as one.
        num_ctx: The context window (``options.num_ctx``).
        top_p: The nucleus sampling cutoff (``options.top_p``).
        top_k: The top-k cutoff (``options.top_k``).
        min_p: The minimum probability cutoff (``options.min_p``).
        api_key: A bearer token, sent as ``Authorization`` over a configured
            one; ``None`` sends ``OLLAMA_API_KEY``'s, where it holds one and
            no authorization is configured.
        headers: Headers sent with every request.

    ``repr`` never shows the key, the value of a header, nor more of
    ``host`` than its scheme, its host and its port.
    """

    host: str = "http://localhost:11434"
    model: str = "llama3.2"
    max_tokens: int | None = None
    timeout: float = 120.0
    connect_timeout: float = 5.0
    max_retries: int = 0
    think: bool | Literal["low", "medium", "high"] | None = None
    keep_alive: str | int | None = None
    num_ctx: int | None = None
    top_p: float | None = None
    top_k: int | None = None
    min_p: float | None = None
    api_key: SecretStr | None = None
    headers: dict[str, str] | None = None

    @field_validator("keep_alive", mode="before")
    @classmethod
    def _seconds_written_as_text(cls, value: object) -> object:
        """A text of a whole number, as RoomKit reads it: that number of
        seconds, Ollama reading a text as a duration that needs a unit."""
        if isinstance(value, str):
            try:
                return int(value.strip())
            except ValueError:
                return value
        return value


class Ollama(Provider[OllamaConfig | RoomKitOllamaConfig], config=OllamaConfig):
    """An Ollama model, as a RoomKit provider: a whole answer asked whole, a
    stream read as JSON lines, a turn's reasoning as Ollama's ``think``, a
    tool's schema sent whole, an image as base64, a path read from the disk.

    ``config`` is this module's ``OllamaConfig``, or RoomKit's own, for a
    host that moves from RoomKit's ``OllamaAIProvider``. It is read once,
    when the provider is built. There is no ``transport`` parameter: the
    provider builds its own HTTP client.

    The environment of the process is read then too, and only then, for
    what RoomKit's provider reads there of the host's network: the proxies
    and the certificate authorities, and the key ``OLLAMA_API_KEY`` holds.

    Cancelling the task that awaits a call stops its request and closes its
    connection; so does closing a stream, or leaving it unread.
    """
