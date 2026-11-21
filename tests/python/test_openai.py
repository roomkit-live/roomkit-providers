"""The OpenAI provider as a Python program holds it: its configuration,
what it says of itself, what a call gives and how it fails."""

from __future__ import annotations

import asyncio
import dataclasses
import json
import tomllib
import unittest
from pathlib import Path

import roomkit_providers
from fake_endpoint import JSON, Fake, Whole, completion
from roomkit_providers import ProviderError, ResponseSchemaError
from roomkit_providers.openai import OpenAIAIProvider, OpenAIConfig

ROOT = Path(__file__).resolve().parents[2]
KEY = "placeholder-not-a-key"
HELLO = {"messages": [{"role": "user", "content": "Hello"}]}


class ConfigTests(unittest.TestCase):
    def test_the_defaults_are_those_of_roomkit_s_configuration(self) -> None:
        config = OpenAIConfig(api_key=KEY, model="gpt-4o")
        self.assertEqual(
            dataclasses.asdict(config),
            {
                "api_key": KEY,
                "base_url": None,
                "model": "gpt-4o",
                "max_tokens": 1024,
                "timeout": 30.0,
                "connect_timeout": 5.0,
                "max_retries": 0,
                "include_stream_usage": True,
                "use_max_completion_tokens": False,
                "supports_custom_temperature": True,
                "reasoning_effort": None,
                "default_headers": None,
                "extra_body": None,
                "supports_response_schema": None,
                "supports_response_schema_with_tools": None,
                "supports_reasoning_effort_with_tools": None,
                "thinking_field": None,
            },
        )

    def test_a_modern_model_is_profiled_on_openai_s_own_endpoint(self) -> None:
        def switches(**members: object) -> tuple[bool | None, bool | None]:
            config = OpenAIConfig(api_key=KEY, **members)
            return config.use_max_completion_tokens, config.supports_custom_temperature

        self.assertEqual(switches(model="gpt-5.1"), (True, False))
        self.assertEqual(switches(model="gpt-4.1"), (True, True))
        self.assertEqual(switches(model="gpt-4o"), (False, True))
        local = "http://localhost:11434/v1"
        self.assertEqual(switches(model="gpt-5.1", base_url=local), (False, True))
        stated = switches(
            model="gpt-5.1", use_max_completion_tokens=False, supports_custom_temperature=True
        )
        self.assertEqual(stated, (False, True))

    def test_a_value_that_does_not_fit_is_refused_when_the_configuration_is_built(self) -> None:
        refused = [
            {"timeout": "30"},
            {"timeout": -1.0},
            {"timeout": float("nan")},
            {"max_tokens": 1024.5},
            {"max_tokens": -5},
            {"max_retries": True},
            {"include_stream_usage": 1},
            {"default_headers": {"x-token": 5}},
            {"extra_body": [1, 2]},
            {"base_url": 8080},
            {"api_key": 5},
            {"api_key": None},
            {"model": None},
        ]
        for members in refused:
            with self.subTest(members=members), self.assertRaises(ValueError):
                OpenAIConfig(**{"api_key": KEY, "model": "gpt-4o", **members})
        with self.assertRaises(TypeError):
            OpenAIConfig(api_key=KEY, model="gpt-4o", vision=True)
        with self.assertRaises(TypeError):
            OpenAIConfig(api_key=KEY)
        with self.assertRaises(TypeError):
            OpenAIConfig(KEY, "gpt-4o")

    def test_what_refuses_a_configuration_does_not_repeat_its_key(self) -> None:
        with self.assertRaises(ValueError) as refused:
            OpenAIConfig(api_key=KEY, model="gpt-4o", timeout="30")
        self.assertNotIn(KEY, str(refused.exception))

    def test_a_configuration_does_not_change(self) -> None:
        headers, body = {"x-token": "placeholder-token"}, {"top_k": [1]}
        config = OpenAIConfig(api_key=KEY, model="m", default_headers=headers, extra_body=body)
        with self.assertRaises(dataclasses.FrozenInstanceError):
            config.model = "another"
        # What the caller does of its own dicts afterwards changes nothing.
        headers["x-more"] = "more"
        body["top_k"].append(2)
        self.assertEqual(config.default_headers, {"x-token": "placeholder-token"})
        self.assertEqual(config.extra_body, {"top_k": [1]})

    def test_repr_shows_no_key_no_header_value_and_no_credentials_of_the_url(self) -> None:
        config = OpenAIConfig(
            api_key=KEY,
            model="local-model",
            base_url="https://user:placeholder-in-the-url@llm.example/v1",
            default_headers={"Authorization": "Basic placeholder-in-a-header"},
        )
        shown = repr(config)
        self.assertNotIn("placeholder", shown)
        self.assertIn("api_key='**********'", shown)
        self.assertIn("base_url='https://**********@llm.example/v1'", shown)
        self.assertIn("default_headers={'Authorization': '**********'}", shown)
        self.assertIn("api_key=''", repr(OpenAIConfig(api_key="", model="local-model")))
        # A URL written without its scheme shows nothing of what precedes
        # its last `@` either.
        odd = OpenAIConfig(api_key=KEY, model="m", base_url="user:placeholder-in-the-url@host")
        self.assertIn("base_url='**********@host'", repr(odd))
        self.assertNotIn("placeholder", repr(odd))
        self.assertEqual(config, dataclasses.replace(config))


class ProviderTests(unittest.TestCase):
    def test_what_a_provider_says_of_itself(self) -> None:
        provider = OpenAIAIProvider(OpenAIConfig(api_key=KEY, model="gpt-4o"))
        said = {
            "name": provider.name,
            "model_name": provider.model_name,
            "supports_vision": provider.supports_vision,
            "supports_streaming": provider.supports_streaming,
            "supports_structured_streaming": provider.supports_structured_streaming,
            "supports_response_schema": provider.supports_response_schema,
            "supports_response_schema_with_tools": provider.supports_response_schema_with_tools,
            "supports_deferred_tools": provider.supports_deferred_tools,
            "context_window": provider.context_window,
        }
        self.assertEqual(
            said,
            {
                "name": "OpenAIAIProvider",
                "model_name": "gpt-4o",
                "supports_vision": True,
                "supports_streaming": True,
                "supports_structured_streaming": True,
                "supports_response_schema": True,
                "supports_response_schema_with_tools": True,
                "supports_deferred_tools": False,
                "context_window": 128000,
            },
        )
        entry = provider.catalog_entry()
        self.assertEqual((entry["id"], entry["display_name"]), ("gpt-4o", "GPT-4o"))

    def test_a_model_the_catalogue_does_not_know(self) -> None:
        local = OpenAIConfig(api_key=KEY, model="local-model", base_url="http://localhost:1/v1")
        provider = OpenAIAIProvider(local)
        self.assertIsNone(provider.catalog_entry())
        self.assertIsNone(provider.context_window)
        self.assertTrue(provider.supports_vision)
        self.assertFalse(provider.supports_response_schema_with_tools)

    def test_the_catalogue_is_read_without_a_provider(self) -> None:
        models = OpenAIAIProvider.available_models()
        self.assertIn("gpt-4o", [model["id"] for model in models])
        self.assertEqual(
            list(models[0]),
            [
                "id",
                "display_name",
                "context_window",
                "supports_vision",
                "deprecated",
                "capabilities",
                "pricing",
            ],
        )
        # Each call gives a list of its own.
        models.clear()
        self.assertTrue(OpenAIAIProvider.available_models())

    def test_a_provider_is_built_from_a_configuration_only(self) -> None:
        with self.assertRaises(TypeError):
            OpenAIAIProvider({"api_key": KEY, "model": "gpt-4o"})

    def test_a_configuration_the_provider_cannot_be_built_with(self) -> None:
        config = OpenAIConfig(api_key=KEY, model="m", base_url="https://llm.example/v1?route=1")
        with self.assertRaises(ProviderError) as refused:
            OpenAIAIProvider(config)
        error = refused.exception
        self.assertTrue(error.before_request)
        self.assertFalse(error.retryable)
        self.assertEqual(error.provider, "openai")
        self.assertIn("its base URL has a query or a fragment", error.message)

    def test_the_version_is_the_project_s(self) -> None:
        project = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))["project"]
        self.assertEqual(roomkit_providers.__version__, project["version"])


class CallTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.fake = await Fake().start()
        self.addAsyncCleanup(self.fake.stop)

    def provider(self, **members: object) -> OpenAIAIProvider:
        base_url = f"http://127.0.0.1:{self.fake.port}/v1"
        config = {"api_key": KEY, "model": "local-model", "base_url": base_url, **members}
        return OpenAIAIProvider(OpenAIConfig(**config))

    async def test_a_request_is_a_plain_dict_and_so_is_its_answer(self) -> None:
        self.fake.answer(Whole(200, JSON, completion("Hello there.")))
        response = await self.provider().generate(HELLO)
        self.assertEqual(
            response,
            {
                "content": "Hello there.",
                "thinking": None,
                "thinking_signature": None,
                "thinking_parts": None,
                "finish_reason": "stop",
                "usage": {"input_tokens": 12, "output_tokens": 5},
                "metadata": {"model": "local-model"},
                "tool_calls": [],
            },
        )
        (seen,) = self.fake.seen
        self.assertEqual(seen.headers["authorization"], f"Bearer {KEY}")
        self.assertEqual(json.loads(seen.body)["messages"], [{"role": "user", "content": "Hello"}])

    async def test_an_image_and_a_tool_cross_in_the_contract_s_form(self) -> None:
        image = {"type": "image", "url": "data:image/png;base64,aGVsbG8=", "mime_type": "image/png"}
        tool = {"name": "lookup_order", "description": "Finds an order."}
        request = {
            "messages": [{"role": "user", "content": [{"type": "text", "text": "See."}, image]}],
            "tools": [tool],
        }
        self.fake.answer(Whole(200, JSON, completion("Seen.")))
        self.assertEqual((await self.provider().generate(request))["content"], "Seen.")
        body = json.loads(self.fake.seen[0].body)
        self.assertEqual(body["messages"][0]["content"][1]["type"], "image_url")
        self.assertEqual(body["tools"][0]["function"]["name"], "lookup_order")

    async def test_a_key_the_request_carries_is_not_the_one_sent(self) -> None:
        # The OpenAI provider reads no key from a request, as RoomKit's.
        request = {**HELLO, "metadata": {"api_key": "placeholder-of-one-request"}}
        self.fake.answer(Whole(200, JSON, completion("Yes.")))
        await self.provider().generate(request)
        (seen,) = self.fake.seen
        self.assertEqual(seen.headers["authorization"], f"Bearer {KEY}")
        self.assertNotIn("placeholder-of-one-request", seen.body.decode())

    async def test_a_request_that_does_not_read_is_a_value_error(self) -> None:
        provider = self.provider()
        refused = [
            {"messages": [{"role": "nobody", "content": "Hello"}]},
            {"messages": "Hello"},
            {"temperature": "warm"},
            {"tools": [{"name": "not a name", "description": ""}]},
            {"response_schema": {"type": "object", "patternProperties": {}}},
            ["messages"],
            {"temperature": float("inf")},
        ]
        for request in refused:
            with self.subTest(request=request):
                with self.assertRaises(ValueError):
                    await provider.generate(request)
                with self.assertRaises(ValueError):
                    await anext(provider.generate_structured_stream(request))
                with self.assertRaises(ValueError):
                    await anext(provider.generate_stream(request))
        with self.assertRaises(TypeError):
            await provider.generate({"messages": b"Hello"})
        # Nothing was sent for any of them.
        self.assertEqual(self.fake.seen, [])

    async def test_a_refusal_of_the_server_is_a_provider_error_with_its_attributes(self) -> None:
        refusal = {
            "error": {
                "message": f"Incorrect API key provided: {KEY}.",
                "type": "invalid_request_error",
                "code": "invalid_api_key",
            }
        }
        self.fake.answer(Whole(401, JSON, json.dumps(refusal).encode()))
        with self.assertRaises(ProviderError) as failed:
            await self.provider().generate(HELLO)
        error = failed.exception
        said = (
            "the provider answered HTTP 401 (invalid_request_error, invalid_api_key): "
            "Incorrect API key provided: [***]."
        )
        self.assertEqual(error.message, said)
        self.assertEqual(error.args, (said,))
        self.assertEqual(str(error), f"openai (401): {said}")
        self.assertEqual(
            (error.retryable, error.provider, error.status_code, error.context_overflow),
            (False, "openai", 401, None),
        )
        self.assertFalse(error.before_request)
        self.assertNotIsInstance(error, ResponseSchemaError)

    async def test_a_failure_of_a_stream_comes_after_what_was_read(self) -> None:
        overflow = {"error": {"message": "Too long.", "code": "context_length_exceeded"}}
        self.fake.answer(Whole(400, JSON, json.dumps(overflow).encode()))
        stream = self.provider().generate_structured_stream(HELLO)
        with self.assertRaises(ProviderError) as failed:
            await anext(stream)
        self.assertEqual(failed.exception.status_code, 400)
        self.assertIs(failed.exception.context_overflow, True)
        with self.assertRaises(StopAsyncIteration):
            await anext(stream)

    async def test_a_schema_the_provider_cannot_carry_is_refused_before_any_request(self) -> None:
        schema = {
            "type": "object",
            "properties": {"label": {"type": "string"}},
            "required": ["label"],
            "additionalProperties": False,
        }
        request = {**HELLO, "response_schema": schema}
        provider = self.provider(supports_response_schema=False)
        with self.assertRaises(ResponseSchemaError) as refused:
            await provider.generate(request)
        error = refused.exception
        self.assertEqual(error.reason, "unsupported")
        self.assertTrue(error.before_request)
        self.assertFalse(error.retryable)
        self.assertEqual(error.provider, "openai")
        self.assertIsInstance(error, ProviderError)
        self.assertEqual(self.fake.seen, [])

    async def test_closing_a_provider_is_awaited(self) -> None:
        provider = self.provider()
        self.assertIsNone(await provider.close())
        self.assertTrue(asyncio.iscoroutinefunction(provider.close))


class ErrorTests(unittest.TestCase):
    def test_a_provider_error_is_shown_led_by_its_provider_and_its_status(self) -> None:
        self.assertEqual(str(ProviderError("Payment required")), "Payment required")
        named = ProviderError("Payment required", provider="acme", status_code=402)
        self.assertEqual(str(named), "acme (402): Payment required")
        self.assertEqual(str(ProviderError("No.", provider="acme")), "acme: No.")
        # A message that already names its provider is left as it is.
        self.assertEqual(str(ProviderError("ACME said no.", provider="acme")), "ACME said no.")
        self.assertEqual(
            (named.message, named.retryable, named.context_overflow, named.before_request),
            ("Payment required", False, None, False),
        )

    def test_a_response_schema_s_error_derives_the_rest_from_its_reason(self) -> None:
        unsupported = ResponseSchemaError("No.", reason="unsupported", provider="acme")
        self.assertTrue(unsupported.before_request)
        refusal = ResponseSchemaError("No.", reason="refusal")
        said = (refusal.before_request, refusal.retryable, refusal.provider)
        self.assertEqual(said, (False, False, ""))


if __name__ == "__main__":
    unittest.main()
