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
SCOPE = "https://www.googleapis.com/auth/cloud-platform"


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

# -- Application Default Credentials -----------------------------------------------------
#
# A configuration with no key calls as the identity google-auth finds: the
# file GOOGLE_APPLICATION_CREDENTIALS names, else gcloud's, else the
# metadata server's. ``ENVIRONMENT`` says what the case runs in: its files,
# its variables (``{dir}`` names the directory of its files), and whether
# the machine says it is a Google Compute Engine one.

USER = {
    "type": "authorized_user",
    "client_id": "100000000001-test.apps.googleusercontent.com",
    "client_secret": "placeholder-client-secret",
    "refresh_token": "placeholder-refresh-token",
}
BORROWED = "ya29.placeholder-borrowed-token"
IMPERSONATION = (
    "https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/"
    f"{EMAIL}:generateAccessToken"
)


def adc(files: dict[str, Any] | None = None, **stated: Any) -> dict[str, Any]:
    """A configuration with no key, in the environment stated."""
    environment = {"files": files or {}, **stated}
    return {"project": PROJECT, "location": "northamerica-northeast1", "ENVIRONMENT": environment}


def in_variable(content: Any, **stated: Any) -> dict[str, Any]:
    """ADC in the file GOOGLE_APPLICATION_CREDENTIALS names."""
    variables = {"GOOGLE_APPLICATION_CREDENTIALS": "{dir}/adc.json", **stated.pop("variables", {})}
    return adc({"adc.json": content}, variables=variables, **stated)


def in_gcloud(content: Any) -> dict[str, Any]:
    """ADC in gcloud's file, its directory named by CLOUDSDK_CONFIG."""
    files = {"gcloud/application_default_credentials.json": content}
    return adc(files, variables={"CLOUDSDK_CONFIG": "{dir}/gcloud"})


def borrowed(expire: str = "2026-01-01T01:00:00Z") -> Reply:
    """What IAM Credentials answers generateAccessToken with."""
    return answer({"accessToken": BORROWED, "expireTime": expire})


def flavoured() -> Reply:
    """The metadata server's answer to google-auth's ping."""
    return answer({}, **{"Metadata-Flavor": "Google"})


def served_by_the_machine() -> tuple[Reply, ...]:
    """What the metadata server answers after the ping: the project, the
    machine's account, then its token."""
    flavour = {"Metadata-Flavor": "Google"}
    project = Reply(status=200, headers={"content-type": "text/plain", **flavour}, text=PROJECT)
    account = {"aliases": ["default"], "email": EMAIL, "scopes": [SCOPE]}
    return (project, answer(account, **flavour), token())


case("adc_key_in_the_variable", "adc", GENERATE, in_variable(keyed()), HELLO, token(), HELLO_THERE)
case("adc_user_in_the_variable", "adc", GENERATE, in_variable(USER), HELLO, token(), HELLO_THERE)
case(
    "adc_user_with_a_quota_project",
    "adc",
    GENERATE,
    in_variable({**USER, "quota_project_id": "quota-1"}),
    HELLO,
    token(),
    HELLO_THERE,
)
case("adc_user_in_gcloud", "adc", GENERATE, in_gcloud(USER), HELLO, token(), HELLO_THERE)
case(
    "adc_user_refused",
    "adc failure",
    GENERATE,
    in_variable(USER),
    HELLO,
    answer({"error": "invalid_grant", "error_description": "Token has been expired or revoked."}, status=400),
)
case(
    "adc_impersonated_in_the_variable",
    "adc impersonation",
    GENERATE,
    in_variable(
        {
            "type": "impersonated_service_account",
            "service_account_impersonation_url": IMPERSONATION,
            "source_credentials": USER,
            "delegates": [],
        }
    ),
    HELLO,
    token(),
    borrowed(),
    HELLO_THERE,
)
case(
    "adc_external_account",
    "adc refused",
    GENERATE,
    in_variable(
        {
            "type": "external_account",
            "audience": "//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/q",
            "subject_token_type": "urn:ietf:params:oauth:token-type:jwt",
            "token_url": "https://sts.googleapis.com/v1/token",
            "credential_source": {"file": "{dir}/subject.txt"},
        }
    ),
    HELLO,
)
case("adc_file_missing", "adc failure", GENERATE, adc(variables={"GOOGLE_APPLICATION_CREDENTIALS": "{dir}/none.json"}), HELLO)
case("adc_file_of_no_type", "adc failure", GENERATE, in_variable({"client_email": EMAIL}), HELLO)
case(
    "adc_metadata_server",
    "adc metadata",
    GENERATE,
    adc(),
    HELLO,
    flavoured(),
    *served_by_the_machine(),
    HELLO_THERE,
)
case(
    "adc_metadata_server_on_its_own_host",
    "adc metadata",
    GENERATE,
    adc(variables={"GCE_METADATA_HOST": "metadata.example:8080", "GCE_METADATA_IP": "10.0.0.2:8080"}),
    HELLO,
    flavoured(),
    *served_by_the_machine(),
    HELLO_THERE,
)
case(
    "adc_none",
    "adc failure",
    GENERATE,
    adc(),
    HELLO,
    REFUSED,
    REFUSED,
    REFUSED,
)
case(
    "adc_on_a_google_machine",
    "adc metadata",
    GENERATE,
    adc(product_name="Google Compute Engine"),
    HELLO,
    REFUSED,
    REFUSED,
    REFUSED,
    *served_by_the_machine(),
    HELLO_THERE,
)

# -- A borrowed service account ------------------------------------------------------------

case(
    "impersonation_with_a_key",
    "impersonation",
    GENERATE,
    vertex(impersonate_service_account=EMAIL),
    HELLO,
    token(),
    borrowed(),
    HELLO_THERE,
)
case(
    "impersonation_with_adc",
    "impersonation adc",
    GENERATE,
    {**in_variable(USER), "impersonate_service_account": EMAIL},
    HELLO,
    token(),
    borrowed(),
    HELLO_THERE,
)
case(
    "impersonation_refused",
    "impersonation failure",
    GENERATE,
    vertex(impersonate_service_account=EMAIL),
    HELLO,
    token(),
    failed(403, "Permission 'iam.serviceAccounts.getAccessToken' denied.", "PERMISSION_DENIED"),
)
case(
    "impersonation_with_no_identity",
    "impersonation failure",
    GENERATE,
    {**adc(), "impersonate_service_account": EMAIL},
    HELLO,
    REFUSED,
    REFUSED,
    REFUSED,
)
