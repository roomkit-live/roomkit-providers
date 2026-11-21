"""No frame on a raising path shows a secret.

A traceback written with its frames' locals, as a debugger and many loggers
write one, is the ``repr`` of every local of every frame the exception went
through. A configuration that is refused, and a call that fails, must leave
none of the configuration's secrets there: the key, a header's value, the
password and the token of a base URL, nor the key a context lends a request.
"""

from __future__ import annotations

import json
import traceback
import unittest
from typing import Any

from fake_endpoint import JSON, Fake, Refused, Whole, local_config
from roomkit.providers.ai import AIContext, AIMessage, ProviderError
from roomkit.providers.openai import OpenAIConfig as RoomKitConfig
from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

KEY = "placeholder-not-a-key"
HEADER = "placeholder-header-value"
PASSWORD = "placeholder-password"
TOKEN = "placeholder-token"
BODY = "placeholder-body-value"
ONE_REQUEST_KEY = "placeholder-of-one-request"
SECRETS = (KEY, HEADER, PASSWORD, TOKEN, BODY, ONE_REQUEST_KEY)

# What every configuration below holds, a secret in each place one goes.
HELD = {
    "api_key": KEY,
    "model": "local-model",
    "base_url": f"https://team:{PASSWORD}@llm.example/v1",
    "default_headers": {"X-Team-Token": HEADER},
    "extra_body": {"session": BODY},
}
# Configurations RoomKit's class takes and no provider is built with.
REFUSED = {
    "a time below zero": {"timeout": -1.0},
    "a connection time below zero": {"connect_timeout": -0.5},
    "a time that is no number": {"timeout": float("nan")},
    "a time without end": {"timeout": float("inf")},
    "retries below zero": {"max_retries": -1},
    "tokens below zero": {"max_tokens": -5},
    "tokens past the largest": {"max_tokens": 2**40},
    "a base URL with a query": {"base_url": f"https://llm.example/v1?token={TOKEN}"},
    "a base URL with a fragment": {"base_url": f"https://llm.example/v1#{TOKEN}"},
    "a base URL with white space": {"base_url": f" https://team:{PASSWORD}@llm.example/v1"},
    "a base URL of another scheme": {"base_url": f"ftp://team:{PASSWORD}@llm.example/{TOKEN}"},
    "a base URL that is none": {"base_url": f"{TOKEN} is no URL"},
    "a header's value on two lines": {"default_headers": {"X-Team-Token": f"{HEADER}\nmore"}},
    "a header's name with a space": {"default_headers": {"X Team Token": HEADER}},
    "a body with no JSON form": {"extra_body": {"session": BODY, "handle": object()}},
}


def written(error: BaseException) -> str:
    """Everything a traceback of `error` writes with its frames' locals, the
    frames of this file left out: a test holds what it tests with."""
    held = traceback.TracebackException.from_exception(error, capture_locals=True)
    seen, chain = [], [held]
    while chain:
        link = chain.pop()
        seen.append(str(link))
        seen += [
            f"{frame.filename}:{frame.name}:{frame.locals}"
            for frame in link.stack
            if frame.filename != __file__
        ]
        chain += [other for other in (link.__cause__, link.__context__) if other is not None]
    return "\n".join(seen)


class RefusedConfigurationTests(unittest.TestCase):
    def refusal(self, config: Any) -> tuple[BaseException, str]:
        """What refuses `config`, and what its traceback writes: read while
        its frames still hold their locals, which `assertRaises` clears."""
        try:
            OpenAI(config)
        except (ValueError, TypeError, ProviderError) as refused:
            return refused, written(refused)
        self.fail("the configuration was not refused")

    def test_a_refused_configuration_leaves_no_secret_in_a_frame(self) -> None:
        for label, members in REFUSED.items():
            for config in (OpenAIConfig, RoomKitConfig):
                with self.subTest(refused=label, config=config.__module__):
                    _, shown = self.refusal(config(**{**HELD, **members}))
                    for secret in SECRETS:
                        self.assertNotIn(secret, shown)

    def test_a_configuration_of_another_provider_leaves_none_either(self) -> None:
        class Derived(RoomKitConfig):
            region: str = "eu"

        error, shown = self.refusal(Derived(**HELD))
        self.assertIsInstance(error, TypeError)
        for secret in SECRETS:
            self.assertNotIn(secret, shown)


# Code of another file than this one, whose frames the search reads: a
# function that fails holding a secret, one that fails of that failure, and
# one that fails while it handles it.
ELSEWHERE = """
def holding(secret):
    raise ValueError("refused")

def caused():
    try:
        holding(SECRET)
    except ValueError as refused:
        raise RuntimeError("failed") from refused

def while_handling():
    try:
        holding(SECRET)
    except ValueError:
        raise RuntimeError("failed") from None

def chained():
    try:
        holding(SECRET)
    except ValueError:
        raise RuntimeError("failed")
"""


class SearchTests(unittest.TestCase):
    """The search itself, held to what it says it reads: the locals of every
    frame of a failure and of what caused it, this file's own left out."""

    def failure(self, raising: str) -> BaseException:
        elsewhere: dict[str, Any] = {"SECRET": KEY}
        exec(compile(ELSEWHERE, "elsewhere.py", "exec"), elsewhere)  # noqa: S102
        try:
            elsewhere[raising]() if raising != "holding" else elsewhere[raising](KEY)
        except (ValueError, RuntimeError) as failed:
            return failed
        self.fail("nothing failed")

    def test_the_search_reads_the_locals_of_a_frame(self) -> None:
        self.assertIn(KEY, written(self.failure("holding")))

    def test_the_search_follows_what_a_failure_was_raised_from(self) -> None:
        self.assertIn(KEY, written(self.failure("caused")))

    def test_the_search_follows_what_a_failure_was_raised_while_handling(self) -> None:
        self.assertIn(KEY, written(self.failure("chained")))

    def test_the_search_follows_what_a_failure_set_aside(self) -> None:
        # What `from None` sets aside is still what the failure came from:
        # its frames are read too.
        self.assertIn(KEY, written(self.failure("while_handling")))

    def test_the_search_leaves_out_the_frames_of_this_file(self) -> None:
        def holding(secret: str) -> None:
            raise ValueError("refused")

        try:
            holding(KEY)
        except ValueError as failed:
            self.assertNotIn(KEY, written(failed))


class FailingCallTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.fake = await Fake().start()
        self.addAsyncCleanup(self.fake.stop)
        settings = {
            **local_config(self.fake.port),
            "default_headers": HELD["default_headers"],
            "extra_body": HELD["extra_body"],
        }
        self.provider = OpenAI(OpenAIConfig(**settings))

    async def failures(self, context: AIContext) -> list[str]:
        """What the traceback of a whole answer's failure writes, and of a
        stream's: read while its frames still hold their locals."""
        shown = []
        for call in (
            self.provider.generate(context),
            anext(self.provider.generate_structured_stream(context)),
        ):
            try:
                await call
            except ProviderError as failed:
                shown.append(written(failed))
        self.assertEqual(len(shown), 2, "a call did not fail")
        return shown

    async def test_a_failing_call_leaves_no_secret_in_a_frame(self) -> None:
        def context(role: str = "user", **metadata: Any) -> AIContext:
            metadata["api_key"] = ONE_REQUEST_KEY
            return AIContext(messages=[AIMessage(role=role, content="Hello")], metadata=metadata)

        # The server repeats what it was sent, as some do.
        said = f"Bad key {KEY}, header {HEADER}, key of the request {ONE_REQUEST_KEY}"
        echo = {"error": {"message": said, "type": KEY}}
        refused = Whole(401, JSON, json.dumps(echo).encode())
        cases = {
            "a refusal that repeats the secrets": ([refused, refused], context()),
            "a connection that is refused": ([Refused(), Refused()], context()),
            "a request the contract refuses": ([], context(role="developer")),
            "a context with no JSON form": (
                [],
                AIContext(
                    messages=[
                        AIMessage(role="user", content="Hello", metadata={"handle": object()})
                    ],
                    metadata={"api_key": ONE_REQUEST_KEY},
                ),
            ),
        }
        for label, (replies, asked) in cases.items():
            with self.subTest(failing=label):
                self.fake.answer(*replies)
                for shown in await self.failures(asked):
                    for secret in SECRETS:
                        self.assertNotIn(secret, shown)


if __name__ == "__main__":
    unittest.main()
