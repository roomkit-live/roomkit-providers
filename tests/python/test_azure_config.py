"""Azure's configuration is RoomKit's AzureAIConfig: the same members, the
same defaults and the same form, its endpoint reduced to the resource's base
as RoomKit's reduces it, by the provider's own reading. A provider takes
RoomKit's own configuration in place of the package's."""

from __future__ import annotations

import unittest

from pydantic import ValidationError
from roomkit.providers.azure import AzureAIConfig as RoomKitAzureAIConfig
from roomkit_providers.llm.azure import AzureAI, AzureAIConfig

KEY = "placeholder-not-a-key"
REQUIRED = {"api_key": KEY, "azure_endpoint": "https://acme.openai.azure.com", "model": "chat-prod"}

# Endpoints as Azure hands them out and as they are pasted.
PASTED = [
    "https://acme.openai.azure.com/openai/v1/",
    "https://acme.openai.azure.com/openai/deployments/chat-prod/chat/completions"
    "?api-version=2024-10-21",
    "  HTTPS://Acme.OpenAI.Azure.com/openai/responses  ",
    "https://gateway.example/llm/openai/",
    "acme.openai.azure.com/openai/v1",
    "",
]


class AzureConfigTests(unittest.TestCase):
    def test_the_form_is_roomkit_s(self) -> None:
        for endpoint in ["https://acme.openai.azure.com", *PASTED]:
            with self.subTest(endpoint=endpoint):
                given = {**REQUIRED, "azure_endpoint": endpoint}
                ours = AzureAIConfig(**given).model_dump(mode="json")
                theirs = RoomKitAzureAIConfig(**given).model_dump(mode="json")
                self.assertEqual(list(ours), list(theirs))
                self.assertEqual(ours, theirs)

    def test_an_endpoint_the_reader_refuses_is_refused_without_repeating_it(self) -> None:
        endpoint = "http://user:secret@[::1/openai"
        with self.assertRaises(ValidationError):
            RoomKitAzureAIConfig(**{**REQUIRED, "azure_endpoint": endpoint})
        with self.assertRaises(ValidationError) as refused:
            AzureAIConfig(**{**REQUIRED, "azure_endpoint": endpoint})
        self.assertNotIn("secret", str(refused.exception))

    def test_its_repr_shows_no_credential_of_the_endpoint(self) -> None:
        endpoint = "https://user:secret@acme.openai.azure.com/openai/v1"
        shown = repr(AzureAIConfig(**{**REQUIRED, "azure_endpoint": endpoint}))
        self.assertNotIn("secret", shown)
        self.assertNotIn(KEY, shown)
        self.assertIn("https://acme.openai.azure.com", shown)

    def test_a_provider_takes_roomkit_s_configuration_and_names_itself_as_roomkit(self) -> None:
        built = AzureAI(RoomKitAzureAIConfig(**REQUIRED))
        self.assertEqual(built.name, "AzureAIProvider")
        self.assertEqual(built.model_name, "chat-prod")
        self.assertTrue(AzureAI(AzureAIConfig(**REQUIRED)).supports_vision)


if __name__ == "__main__":
    unittest.main()
