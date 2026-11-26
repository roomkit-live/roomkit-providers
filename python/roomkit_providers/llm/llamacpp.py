"""Managed local inference on a GGUF model through llama.cpp."""

from __future__ import annotations

from pydantic import Field, field_validator
from roomkit.providers.llamacpp import LlamaCppConfig as RoomKitLlamaCppConfig

from ._config import Config
from ._provider import ManagedServer, Provider

__all__ = ["LlamaCpp", "LlamaCppConfig"]


class LlamaCppConfig(Config, vendor="llamacpp", stands_for=RoomKitLlamaCppConfig):
    """Select a GGUF file or a Hugging Face reference and configure its local server."""

    model: str
    context_size: int = Field(default=8192, gt=0)
    gpu_layers: int | None = Field(default=None, ge=0)
    max_tokens: int = Field(default=1024, gt=0)
    enable_thinking: bool | None = None
    binary: str | None = None
    variant: str | None = None
    cache_dir: str | None = None
    port: int | None = Field(default=None, gt=0, lt=65536)
    startup_timeout: float = Field(default=1800.0, gt=0)
    timeout: float = Field(default=120.0, gt=0)
    extra_args: list[str] = Field(default_factory=list, repr=False)

    @field_validator("model")
    @classmethod
    def _model(cls, value: str) -> str:
        if not value.strip():
            raise ValueError("model must name a GGUF file or a Hugging Face repository")
        return value

    @field_validator("variant")
    @classmethod
    def _variant(cls, value: str | None) -> str | None:
        return RoomKitLlamaCppConfig(model="local", variant=value).variant


class LlamaCpp(
    ManagedServer, Provider[LlamaCppConfig | RoomKitLlamaCppConfig], config=LlamaCppConfig
):
    """Run a GGUF model locally and release its process on close."""
