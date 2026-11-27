"""Gemini on Vertex AI: its configuration is RoomKit's GeminiVertexConfig,
its provider takes RoomKit's own configuration and names itself as RoomKit's
does, and the wire cases captured on RoomKit's Python provider replay
through it, its service-account key built from the fixtures' test key."""

from __future__ import annotations

import base64
import json
import unittest
from pathlib import Path
from typing import Any

from pydantic import ValidationError
from replay_cases import Replays, every_case
from roomkit.providers.gemini import GeminiVertexConfig as RoomKitGeminiVertexConfig
from roomkit_providers.llm.vertex import GeminiVertex, GeminiVertexConfig

FIXTURES = Path(__file__).resolve().parents[2] / "crates/roomkit-providers-gemini/tests/fixtures"
VERTEX = {"project": "proj-1", "location": "northamerica-northeast1"}


def test_key_pem() -> str:
    """The PEM of the fixtures' test key, as a key file writes it."""
    der = base64.b64encode((FIXTURES / "test-only-rsa-pkcs8.der").read_bytes()).decode()
    lines = "\n".join(der[at : at + 64] for at in range(0, len(der), 64))
    return f"-----BEGIN PRIVATE KEY-----\n{lines}\n-----END PRIVATE KEY-----\n"


def key_file() -> str:
    """A service-account key on the fixtures' test key."""
    pem = test_key_pem()
    return json.dumps(
        {
            "type": "service_account",
            "client_email": "runner@proj-1.iam.gserviceaccount.com",
            "token_uri": "https://oauth2.googleapis.com/token",
            "private_key": pem,
        }
    )


def keyed(config: dict[str, Any]) -> dict[str, Any]:
    """A case's configuration with its key file written whole, as the
    capture built it: the members the fixture gives under ``KEYED``, and the
    test key."""
    key = config.get("service_account_json")
    if not isinstance(key, dict):
        return config
    members = {**key["KEYED"], "private_key": test_key_pem()}
    return {**config, "service_account_json": json.dumps(members)}


GEMINI_VERTEX = Replays(
    vendor="vertex",
    provider=GeminiVertex,
    config=GeminiVertexConfig,
    cases="python_vertex_cases.json",
    key_header="authorization",
    crate="gemini",
    named_cases="vertex_named_cases.json",
    prepared=keyed,
    # A case of Application Default Credentials runs in an environment of
    # its own (its files, its variables), which the Rust replay gives the
    # provider; the native module reads the process's.
    takes=lambda case: "ENVIRONMENT" not in case["config"],
)


class GeminiVertexConfigTests(unittest.TestCase):
    def test_the_form_is_roomkit_s(self) -> None:
        for given in (
            VERTEX,
            {**VERTEX, "location": "global", "labels": {"tenant": "acme", "équipe": "nord-1"}},
            {**VERTEX, "model": "gemini-2.5-flash", "service_account_json": "placeholder"},
        ):
            with self.subTest(given=sorted(given)):
                ours = GeminiVertexConfig(**given).model_dump(mode="json")
                theirs = RoomKitGeminiVertexConfig(**given).model_dump(mode="json")
                self.assertEqual(list(ours), list(theirs))
                self.assertEqual(ours, theirs)

    def test_labels_google_refuses_are_refused_as_roomkit_refuses_them(self) -> None:
        for labels in ({"Tenant": "acme"}, {"tenant": "a b"}, {"チーム": "v"}):
            with self.subTest(labels=labels):
                with self.assertRaises(ValidationError):
                    RoomKitGeminiVertexConfig(**VERTEX, labels=labels)
                with self.assertRaises(ValidationError):
                    GeminiVertexConfig(**VERTEX, labels=labels)

    def test_its_repr_shows_no_key_file(self) -> None:
        shown = repr(GeminiVertexConfig(**VERTEX, service_account_json=key_file()))
        self.assertNotIn("PRIVATE KEY", shown)

    def test_a_provider_takes_roomkit_s_configuration_and_names_itself_as_roomkit(self) -> None:
        built = GeminiVertex(RoomKitGeminiVertexConfig(**VERTEX, service_account_json=key_file()))
        self.assertEqual(built.name, "GeminiVertexProvider")
        self.assertEqual(built.model_name, "gemini-3.8-flash")
        self.assertTrue(built.supports_vision)


class GeminiVertexReplayTests(unittest.IsolatedAsyncioTestCase):
    async def test_every_case_replays_as_python_or_is_named(self) -> None:
        counts, wrong = await every_case(GEMINI_VERTEX)
        self.assertEqual(wrong, [], "\n".join(wrong))
        self.assertEqual(
            counts,
            {
                ("generate", "as Python"): 16,
                ("generate", "named"): 5,
                ("generate_structured_stream", "as Python"): 1,
                ("list_models", "as Python"): 1,
                ("list_models", "named"): 1,
            },
        )


if __name__ == "__main__":
    unittest.main()
