"""The failures of a provider, as RoomKit's provider errors are."""

from __future__ import annotations

import re
from typing import Literal

ResponseSchemaFailure = Literal["unsupported", "refusal", "truncated", "invalid_json"]


class ProviderError(Exception):
    """A failure of a call to a model provider.

    Attributes:
        message: What failed. A secret of the configuration that a server
            sent back is replaced by ``[***]``.
        retryable: Whether a new request may succeed.
        provider: The name of the provider that failed.
        status_code: The HTTP status the provider answered, when it answered.
        context_overflow: Whether the request was past the model's context
            window. ``True`` and ``False`` are stated by what the provider
            read; ``None`` means nothing stated it.
        before_request: Whether the provider refused before it sent
            anything, for something other than what the request carries.
    """

    def __init__(
        self,
        message: str,
        *,
        retryable: bool = False,
        provider: str = "",
        status_code: int | None = None,
        context_overflow: bool | None = None,
        before_request: bool = False,
    ) -> None:
        super().__init__(message)
        self.message = message
        self.retryable = retryable
        self.provider = provider
        self.status_code = status_code
        self.context_overflow = context_overflow
        self.before_request = before_request

    def __str__(self) -> str:
        """The message, led by the provider and the status it answered:
        ``openai (401): the provider answered HTTP 401``. A message that
        already names its provider is left as it is."""
        message = super().__str__()
        if not self.provider or re.search(
            rf"\b{re.escape(self.provider)}\b", message, re.IGNORECASE
        ):
            return message
        status = f" ({self.status_code})" if self.status_code is not None else ""
        return f"{self.provider}{status}: {message}"


class ResponseSchemaError(ProviderError):
    """A response schema was set and no answer satisfying it can be returned.

    Never retryable: the same request fails the same way.

    Attributes:
        reason: ``"unsupported"`` when the call cannot carry a schema, which
            is said before any request is sent; ``"refusal"`` when the model
            declined to answer; ``"truncated"`` when the output cap or the
            context window cut the document; ``"invalid_json"`` when the text
            is no JSON document satisfying the schema.
    """

    def __init__(self, message: str, *, reason: ResponseSchemaFailure, provider: str = "") -> None:
        super().__init__(
            message, retryable=False, provider=provider, before_request=reason == "unsupported"
        )
        self.reason: ResponseSchemaFailure = reason
