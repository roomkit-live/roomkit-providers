"""Google's Gemini models on Vertex AI, in a Google Cloud project's region.

    from roomkit_providers.llm.vertex import GeminiVertex, GeminiVertexConfig

    provider = GeminiVertex(
        GeminiVertexConfig(
            project="my-project",
            location="northamerica-northeast1",
            service_account_json=key_file_contents,
        )
    )

``GeminiVertex`` is a RoomKit ``AIProvider``: an AI channel takes it as it
takes RoomKit's own ``GeminiVertexProvider``.
"""

from __future__ import annotations

from pydantic import SecretStr
from roomkit.providers.gemini import GeminiVertexConfig as RoomKitGeminiVertexConfig

from ._config import Config
from ._provider import Provider

__all__ = ["GeminiVertex", "GeminiVertexConfig"]


class GeminiVertexConfig(
    Config,
    vendor="vertex",
    stands_for=RoomKitGeminiVertexConfig,
    rewritten=("labels",),
    key="service_account_json",
):
    """What a Gemini provider on Vertex AI is configured with: RoomKit's
    ``GeminiVertexConfig``.

    Attributes:
        api_key: Unused on Vertex AI, whose identity is a service account's.
        model: The model to ask; ``gemini-3.8-flash`` when left out, as
            RoomKit's.
        max_tokens: The most tokens an answer may hold, which a request's
            own outranks.
        thinking_level: The thinking level of a Gemini 3 model, which a
            request's effort outranks; ``None`` leaves the model's.
        timeout: The time, in seconds, the first read of an answer, and each
            one after it, may take.
        connect_timeout: The time, in seconds, a connection may take.
        project: The Google Cloud project that hosts the Vertex AI API.
        location: The region the requests are processed in, ``global`` or a
            multi-region (``us``, ``eu``): required, so where the data goes
            is a choice.
        service_account_json: A service-account key file's contents, to call
            as that account; ``None`` calls as Application Default
            Credentials, found as google-auth finds them (the file
            ``GOOGLE_APPLICATION_CREDENTIALS`` names, gcloud's file, the
            metadata server). An external account is not taken yet.
        impersonate_service_account: The email of a service account to
            borrow, with the identity above.
        labels: The billing labels every request carries, held to Google's
            rules, as RoomKit's are.

    ``repr`` never shows the key file, nor the key.
    """

    api_key: SecretStr | None = None
    model: str = "gemini-3.8-flash"
    max_tokens: int = 1024
    thinking_level: str | None = None
    timeout: float = 60.0
    connect_timeout: float = 5.0
    project: str
    location: str
    service_account_json: SecretStr | None = None
    impersonate_service_account: str | None = None
    labels: dict[str, str] | None = None


class GeminiVertex(
    Provider[GeminiVertexConfig | RoomKitGeminiVertexConfig], config=GeminiVertexConfig
):
    """A Gemini model on Vertex AI, as a RoomKit provider: the Developer
    API's generation, stream, thinking and catalogue, asked of the host of
    the configuration's location, as a service account, with the billing
    labels.

    ``config`` is this module's ``GeminiVertexConfig``, or RoomKit's own,
    for a host that moves from RoomKit's ``GeminiVertexProvider``. It is read
    once, when the provider is built: the service-account key is read then.
    There is no ``transport`` parameter: the provider builds its own HTTP
    clients.

    The environment of the process is read then too, for what RoomKit's
    provider reads there of the host's network (the proxies and the
    certificate authorities) and of Application Default Credentials (their
    file is read then; the metadata server is asked at the first call).

    Cancelling the task that awaits a call stops its request and closes its
    connection; so does closing a stream, or leaving it unread.
    """
