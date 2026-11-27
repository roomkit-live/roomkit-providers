"""The cases ``generate_vertex_cases.py`` runs on RoomKit's Python Gemini
provider on Vertex AI: where a request goes (the host of a region, of
``global``, of a multi-region; the project and the location in the path),
what it carries beside a Developer API request (the billing labels), how the
identity of a service-account key is asked for a token and the token sent,
what the provider raises for a key it cannot read and for a token refused,
and how it lists the models.

A key is written ``KEYED``: the members of a service-account key file but
its private key, which the generator adds from the test key of the fixtures
(``test-only-rsa-pkcs8.der``), as a replay does.
"""

from __future__ import annotations

from typing import Any

from gemini_cases import HELLO, HELLO_THERE, failed, model, said
from openai_chat_cases import TOOLS
from wire_capture import (
    GENERATE,
    LISTING,
    REFUSED,
    STREAM,
    Case,
    Reply,
    answer,
    page,
    registrar,
)

CASES: list[Case] = []
case = registrar(CASES)

PROJECT = "proj-1"
EMAIL = "runner@proj-1.iam.gserviceaccount.com"
TOKEN = "ya29.placeholder-access-token"


def keyed(**members: Any) -> dict[str, Any]:
    """A service-account key, its private key left to the generator."""
    key = {
        "type": "service_account",
        "project_id": PROJECT,
        "private_key_id": "test-only-key-1",
        "client_email": EMAIL,
        "client_id": "100000000000000000001",
        "token_uri": "https://oauth2.googleapis.com/token",
    }
    return {"KEYED": {**key, **members}}


def vertex(location: str = "northamerica-northeast1", **members: Any) -> dict[str, Any]:
    """A configuration on a key, in ``location``."""
    return {"project": PROJECT, "location": location, "service_account_json": keyed(), **members}


def token(access: str = TOKEN, expires_in: int = 3599) -> Reply:
    """What Google's token endpoint answers an assertion with."""
    return answer({"access_token": access, "expires_in": expires_in, "token_type": "Bearer"})


MONTREAL = vertex()

# -- Where a request goes -------------------------------------------------------

case("regional", "host", GENERATE, MONTREAL, HELLO, token(), HELLO_THERE)
case("regional_stream", "host stream", STREAM, MONTREAL, HELLO, token(), said("Hel", "lo."))
case("global", "host sdk", GENERATE, vertex("global"), HELLO, token(), HELLO_THERE)
case("multi_region_eu", "host sdk", GENERATE, vertex("eu"), HELLO, token(), HELLO_THERE)
case("multi_region_us", "host sdk", GENERATE, vertex("us"), HELLO, token(), HELLO_THERE)
case(
    "another_model",
    "host",
    GENERATE,
    vertex(model="gemini-2.5-flash", max_tokens=256),
    HELLO,
    token(),
    HELLO_THERE,
)

case(
    "a_publisher_s_model",
    "host sdk",
    GENERATE,
    vertex(model="mistralai/mistral-small-2503"),
    HELLO,
    token(),
    HELLO_THERE,
)
case(
    "a_model_named_as_a_resource",
    "host sdk",
    GENERATE,
    vertex(model="models/gemini-2.5-flash"),
    HELLO,
    token(),
    HELLO_THERE,
)
case(
    "a_model_of_a_project",
    "host sdk",
    GENERATE,
    vertex(model="projects/proj-1/locations/northamerica-northeast1/endpoints/42"),
    HELLO,
    token(),
    HELLO_THERE,
)

# -- Labels -----------------------------------------------------------------------

case(
    "labels",
    "labels",
    GENERATE,
    vertex(labels={"tenant": "acme", "équipe": "nord-1", "empty": ""}),
    HELLO,
    token(),
    HELLO_THERE,
)
case("labels_none", "labels", GENERATE, vertex(labels={}), HELLO, token(), HELLO_THERE)
case(
    "labels_beside_a_system_prompt_and_tools",
    "labels tools",
    GENERATE,
    vertex(labels={"tenant": "acme"}),
    {**TOOLS, "system_prompt": "You answer in one sentence."},
    token(),
    HELLO_THERE,
)

# -- The identity of a key ----------------------------------------------------------

case(
    "key_with_its_own_token_uri",
    "identity",
    GENERATE,
    {**MONTREAL, "service_account_json": keyed(token_uri="https://oauth2.example/token")},
    HELLO,
    token(),
    HELLO_THERE,
)
case(
    "key_refused",
    "identity failure",
    GENERATE,
    MONTREAL,
    HELLO,
    answer({"error": "invalid_grant", "error_description": "Invalid JWT Signature."}, status=400),
)
case(
    "key_endpoint_down",
    "identity failure",
    GENERATE,
    MONTREAL,
    HELLO,
    answer({"error": "internal_failure"}, status=503),
    answer({"error": "internal_failure"}, status=503),
    answer({"error": "internal_failure"}, status=503),
)
case("key_endpoint_unreachable", "identity failure", GENERATE, MONTREAL, HELLO, REFUSED)
case("key_token_page", "identity failure", GENERATE, MONTREAL, HELLO, page("<html>Hi</html>", 200))
case(
    "key_token_without_a_lifetime",
    "identity",
    GENERATE,
    MONTREAL,
    HELLO,
    answer({"access_token": TOKEN, "token_type": "Bearer"}),
    HELLO_THERE,
)

# -- Failures of Vertex ----------------------------------------------------------------

case(
    "permission_denied",
    "failure",
    GENERATE,
    MONTREAL,
    HELLO,
    token(),
    failed(403, "Permission 'aiplatform.endpoints.predict' denied.", "PERMISSION_DENIED"),
)
case(
    "model_not_found",
    "failure",
    GENERATE,
    vertex(model="gemini-0-gone"),
    HELLO,
    token(),
    failed(404, "Publisher Model was not found.", "NOT_FOUND"),
)

# -- The listing ------------------------------------------------------------------------


def published(name: str, **more: Any) -> dict[str, Any]:
    """An entry of Vertex's listing: a publisher's model, no actions."""
    entry = model(name, **more)
    entry["name"] = f"publishers/google/models/{name}"
    return entry


case(
    "models_listed",
    "listing",
    LISTING,
    MONTREAL,
    {},
    token(),
    answer(
        {
            "models": [
                published("gemini-3.8-flash"),
                published("gemini-11-new"),
                published("text-embedding-005"),
                published("imagen-5"),
            ]
        }
    ),
)
case(
    "models_listing_failed",
    "listing failure",
    LISTING,
    MONTREAL,
    {},
    token(),
    failed(403, "Permission denied.", "PERMISSION_DENIED"),
)
