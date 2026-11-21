"""A provider's failure as the native module raises it."""

from __future__ import annotations


class ProviderFailure(Exception):
    """The contract's failure of a call, attribute by attribute, before it
    is the exception a caller catches.

    The native module raises it and nothing else builds it. Its text is its
    first argument; ``schema_reason`` is the reason of a response schema's
    failure, and None for any other failure.
    """

    def __init__(
        self,
        message: str,
        *,
        retryable: bool,
        provider: str,
        status_code: int | None,
        context_overflow: bool | None,
        before_request: bool,
        schema_reason: str | None,
    ) -> None:
        super().__init__(message)
        self.retryable = retryable
        self.provider = provider
        self.status_code = status_code
        self.context_overflow = context_overflow
        self.before_request = before_request
        self.schema_reason = schema_reason
