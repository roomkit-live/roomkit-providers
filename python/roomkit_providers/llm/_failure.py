"""A provider's failure as the native module raises it, and as a caller
catches it: RoomKit's own exception."""

from __future__ import annotations

from collections.abc import Iterator
from contextlib import contextmanager

from roomkit.providers.ai import ProviderError, ResponseSchemaError


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


def _as_roomkit(failure: ProviderFailure) -> ProviderError:
    """`failure` as RoomKit's own exception, with the same attributes."""
    if failure.schema_reason is not None:
        return ResponseSchemaError(
            failure.args[0], reason=failure.schema_reason, provider=failure.provider
        )
    return ProviderError(
        failure.args[0],
        retryable=failure.retryable,
        provider=failure.provider,
        status_code=failure.status_code,
        context_overflow=failure.context_overflow,
        before_request=failure.before_request,
    )


@contextmanager
def failing_as_roomkit() -> Iterator[None]:
    """Raises a failure of the native module as RoomKit's exception."""
    try:
        yield
    except ProviderFailure as failure:
        raise _as_roomkit(failure) from None
