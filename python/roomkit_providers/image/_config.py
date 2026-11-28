"""An image provider's configuration: the one every provider shares, the
configuration of ``roomkit_providers.llm``, which a vendor's module of this
kind derives from as one of ``llm`` does."""

from __future__ import annotations

from ..llm._config import Config, Veiled

__all__ = ["Config", "Veiled"]
