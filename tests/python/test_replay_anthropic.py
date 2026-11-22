"""Anthropic's recorded wire cases, replayed through the package's provider.

Each case of ``python_messages_cases.json`` is asked of ``Anthropic`` as
RoomKit asks it, against a local fake of the endpoint that serves each
exchange, a stream read by read at the recorded bytes. What the provider
sent and gave is what the Rust provider sends and gives: for a case the Rust
replay holds as Python, what Python did; for a case it names, what its entry
states.

Left to the Rust replay, which moves its own clock: the cases whose answer
stops under way, and the timeouts each request is given, which no server
sees.
"""

from __future__ import annotations

import unittest

from fake_endpoint import Fake, Seen
from replay_cases import (
    LEFT_TO_RUST,
    NEVER_ANSWERED,
    Replays,
    cases_set_apart,
    differences,
    every_case,
    made_wrong,
    replay,
)
from roomkit.providers.ai import ProviderError
from roomkit_providers.llm.anthropic import Anthropic, AnthropicConfig

ANTHROPIC = Replays(
    vendor="anthropic",
    provider=Anthropic,
    config=AnthropicConfig,
    cases="python_messages_cases.json",
    key_header="x-api-key",
)


class ReplayTests(unittest.IsolatedAsyncioTestCase):
    async def test_every_case_replays_as_the_rust_provider(self) -> None:
        counts, wrong = await every_case(ANTHROPIC)
        self.assertEqual(wrong, [], "\n".join(wrong))
        self.assertEqual(
            counts,
            {
                ("generate", "as Python"): 211,
                ("generate", "named"): 36,
                ("generate", "left to Rust"): 1,
                ("generate_structured_stream", "as Python"): 82,
                ("generate_structured_stream", "named"): 22,
                ("generate_structured_stream", "left to Rust"): 1,
                ("generate_stream", "as Python"): 5,
                ("generate_stream", "named"): 1,
                ("list_models", "as Python"): 9,
                ("list_models", "named"): 9,
            },
        )

    def test_the_cases_set_apart_are_those_whose_fake_goes_silent(self) -> None:
        """A case is left to the Rust replay, or given a short timeout, for
        what its exchanges are, not by its name: these are the ones."""
        self.assertEqual(
            cases_set_apart(ANTHROPIC),
            {
                # The fake stops answering after it answered something, or
                # before a try that is answered.
                LEFT_TO_RUST: {"retry_of_a_timeout", "stream_read_timeout_after_an_event"},
                # The fake never answers their one request.
                NEVER_ANSWERED: {"timeout", "stream_timeout"},
            },
        )

    def test_every_named_case_is_a_case_of_the_fixture(self) -> None:
        names = {case["name"] for case in ANTHROPIC.fixture()["cases"]}
        self.assertEqual(set(ANTHROPIC.named()) - names, set())
        self.assertEqual(len(ANTHROPIC.named()), 68)


class ReplayRefusesTests(unittest.IsolatedAsyncioTestCase):
    """The replay held to what it compares of what the cases of this fixture
    alone show: the key a request carries for itself, the calls made before
    a case's own, whether the provider holds a tool unseen, a request the
    contract does not read, and a listing. A case is replayed, found to be
    as stated, then one thing is made wrong, and the replay must say so.
    What the cases of any fixture show is held the same way in
    ``test_replay``."""

    async def refuses(self, name: str, word: str, wrong=None, **where) -> None:
        self.assertIn(word, await made_wrong(ANTHROPIC, name, wrong, **where), name)

    async def test_a_key_that_is_not_the_one_its_request_carries(self) -> None:
        def the_configuration_s(replayed):
            replayed.seen[0].headers["x-api-key"] = replayed.key

        def a_request_said_to_carry_none(replayed):
            replayed.request_key = None

        def a_key_all_the_same(replayed):
            replayed.seen[0].headers["x-api-key"] = "42"

        sent = 'request 0: x-api-key is Some("placeholder-not-a-key")'
        await self.refuses("key_of_the_request", sent, the_configuration_s)
        carried = 'request 0: x-api-key is Some("placeholder-not-a-key-either")'
        await self.refuses("key_of_the_request", carried, a_request_said_to_carry_none)
        # A key that is no text is none: the configuration's is sent.
        no_text = "key_of_the_request_that_is_no_text"
        await self.refuses(no_text, 'request 0: x-api-key is Some("42")', a_key_all_the_same)

    async def test_a_call_before_the_case_s_own_that_is_not_python_s(self) -> None:
        before = "before the case's own"

        def a_call_less(replayed):
            replayed.before.pop()

        def the_key_of_the_call_after(replayed):
            replayed.before[0].seen[0].headers["x-api-key"] = (
                replayed.before[1].seen[0].headers["x-api-key"]
            )

        def another_text(replayed):
            replayed.before[1].given["content"] += "!"

        def a_request_more(replayed):
            replayed.before[2].seen.append(replayed.before[2].seen[0])

        def a_wait(replayed):
            replayed.before[3].waits.append(0.5)

        def a_failure(replayed):
            replayed.before[0].error = ProviderError("refused")

        def nothing_asked(replayed):
            replayed.before[0].asked = False

        def not_read(replayed):
            replayed.before[0].unread = "no"

        def an_event_less(replayed):
            replayed.before[1].given.pop()

        several = "keys_of_several_requests"
        await self.refuses(several, f"3 calls made {before}, where Python made 4", a_call_less)
        key = f"call 0 {before}: request 0: x-api-key is"
        await self.refuses(several, key, the_key_of_the_call_after)
        await self.refuses(several, f"call 1 {before}: the response", another_text)
        await self.refuses(several, f"call 2 {before}: 2 requests sent", a_request_more)
        await self.refuses(several, f"call 3 {before}: 1 waits", a_wait)
        await self.refuses(several, f"call 0 {before}: a failure where Python", a_failure)
        await self.refuses(several, f"call 0 {before}: nothing was asked", nothing_asked)
        await self.refuses(several, f"call 0 {before}: the request is not read", not_read)
        streams = "stream_keys_of_several_requests"
        await self.refuses(streams, f"call 1 {before}: the events", an_event_less)

    async def test_a_call_takes_the_waits_of_no_call_before_it(self) -> None:
        """No recorded case waits in a call made before its own, so one is
        put together: a case tried again once, asked twice of one provider,
        takes one wait each time."""
        cases = {case["name"]: case for case in ANTHROPIC.fixture()["cases"]}
        tried_again = cases["retry_that_succeeds"]
        twice = {**tried_again, "before": [tried_again]}
        async with Fake() as fake:
            replayed = await replay(ANTHROPIC, twice, fake)
        self.assertEqual(differences(twice, replayed), [])
        self.assertEqual([len(replayed.before[0].waits), len(replayed.waits)], [1, 1])

    async def test_a_tool_held_unseen_where_python_holds_none(self) -> None:
        def another_answer(replayed):
            held = replayed.supports["supports_deferred_tools"]
            replayed.supports["supports_deferred_tools"] = not held

        def no_answer(replayed):
            del replayed.supports["supports_deferred_tools"]

        await self.refuses("plain_answer", "supports_deferred_tools is False", another_answer)
        behind_a_base_url = "tools_held_unseen_behind_a_base_url"
        await self.refuses(behind_a_base_url, "supports_deferred_tools is True", another_answer)
        await self.refuses("plain_answer", "supports_deferred_tools is None", no_answer)

    async def test_a_request_the_contract_does_not_read_that_is_not_as_its_entry_states(
        self,
    ) -> None:
        otherwise = "it differs from Python otherwise than its entry states"

        def another_reason(replayed):
            replayed.unread = "no"

        def a_request_all_the_same(replayed):
            replayed.seen.append(Seen("POST", "/v1/messages", {}, [], b"{}"))

        def worth_a_retry(replayed):
            replayed.error.retryable = True

        def no_failure(replayed):
            replayed.error = None

        def read(replayed):
            replayed.unread = None

        role = "turn_whose_role_the_provider_does_not_know"
        await self.refuses(role, "not {", another_reason)
        await self.refuses(role, "not {", a_request_all_the_same)
        await self.refuses(role, "not {", worth_a_retry)
        await self.refuses(role, "not {", no_failure)
        await self.refuses(role, otherwise, read)
        # A request that is read, held to the entry of one that is not.
        await self.refuses("plain_answer", "named, and it replays as Python", entry_of=role)

    async def test_a_listing_or_a_text_that_is_not_python_s(self) -> None:
        def a_model_less(replayed):
            replayed.given.pop()

        def a_model_not_filled(replayed):
            replayed.given[0]["display_name"] = None

        def another_status(replayed):
            replayed.error.status_code = 403

        def a_piece_less(replayed):
            replayed.given.pop()

        await self.refuses("list_models", "the models", a_model_less)
        await self.refuses("list_models", "the models", a_model_not_filled)
        await self.refuses("list_models_status_401", "not {", another_status)
        await self.refuses("stream_as_text", "the text", a_piece_less)


if __name__ == "__main__":
    unittest.main()
