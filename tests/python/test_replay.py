"""The recorded wire cases, replayed through the package's provider.

Each case of ``python_chat_cases.json`` is asked of ``OpenAI`` as RoomKit
asks it, against a local fake of the endpoint that serves each exchange, a
stream read by read at the recorded bytes. What the provider sent and gave
is what the Rust provider sends and gives: for a case the Rust replay holds
as Python, what Python did; for a case it names, what its entry states.

Left to the Rust replay, which moves its own clock: the cases whose answer
stops under way, and the timeouts each request is given, which no server
sees.
"""

from __future__ import annotations

import copy
import unittest
from collections import Counter

from fake_endpoint import Fake
from replay_cases import fixture, goes_silent, judged, named, replay
from roomkit.providers.ai import ProviderError

# The cases whose fake stops answering after it answered something, or
# before a try that is answered: the provider's timeout ends the silence,
# and what was sent before must be read first. The Rust replay lets that
# time pass on a clock it moves; here the configuration's 30 s would have
# to pass, or a shorter timeout race what was sent.
LEFT_TO_RUST = {
    "retry_of_a_timeout",
    "answer_timed_out_midway_on_every_try",
    "stream_read_timeout_after_an_event",
}

# The cases whose fake never answers their one request: with a timeout of
# 50 ms in place of the configuration's, nothing but the timeout ends them,
# however long anything takes.
NEVER_ANSWERED = {"timeout", "stream_timeout"}
SHORT_TIMEOUT = 0.05


class ReplayTests(unittest.IsolatedAsyncioTestCase):
    async def test_every_case_replays_as_the_rust_provider(self) -> None:
        cases, stated = fixture()["cases"], named()
        counts: Counter[tuple[str, str]] = Counter()
        wrong = []
        async with Fake() as fake:
            for case in cases:
                name, call = case["name"], case["call"]
                if name in LEFT_TO_RUST:
                    counts[call, "left to Rust"] += 1
                    continue
                short = SHORT_TIMEOUT if name in NEVER_ANSWERED else None
                replayed = await replay(case, fake, read_timeout=short)
                as_python, said = judged(case, stated.get(name), replayed)
                if said:
                    wrong.append(f"{name}: {said}")
                counts[call, "as Python" if as_python else "named"] += 1
        self.assertEqual(wrong, [], "\n".join(wrong))
        self.assertEqual(
            dict(counts),
            {
                ("generate", "as Python"): 185,
                ("generate", "named"): 24,
                ("generate", "left to Rust"): 2,
                ("generate_structured_stream", "as Python"): 95,
                ("generate_structured_stream", "named"): 19,
                ("generate_structured_stream", "left to Rust"): 1,
                ("generate_stream", "as Python"): 4,
                ("list_models", "as Python"): 5,
                ("list_models", "named"): 5,
            },
        )

    def test_the_cases_set_apart_are_those_whose_fake_goes_silent(self) -> None:
        """A case is left to the Rust replay, or given a short timeout, for
        what its exchanges are, not by its name."""
        left, never = set(), set()
        for case in fixture()["cases"]:
            silences = [goes_silent(exchange) for exchange in case["exchanges"]]
            if not any(silences):
                continue
            answered = silences == [True] and case["exchanges"][0]["response"].get("failure")
            (never if answered else left).add(case["name"])
        self.assertEqual(left, LEFT_TO_RUST)
        self.assertEqual(never, NEVER_ANSWERED)

    def test_every_named_case_is_a_case_of_the_fixture(self) -> None:
        names = {case["name"] for case in fixture()["cases"]}
        self.assertEqual(set(named()) - names, set())
        self.assertEqual(len(named()), 48)


class ReplayRefusesTests(unittest.IsolatedAsyncioTestCase):
    """The replay held to what it compares: a case is replayed, found to be
    as stated, then one thing is made wrong, of what was replayed, of the
    case or of its entry among the named cases, and the replay must say so.
    Each comparison of ``replay_cases`` has a wrong here that only it
    finds."""

    async def refuses(
        self,
        name: str,
        word: str,
        wrong=None,
        *,
        in_case=None,
        in_entry=None,
        entry_of: str | None = None,
    ) -> None:
        cases = {case["name"]: case for case in fixture()["cases"]}
        case = copy.deepcopy(cases[name])
        stated = copy.deepcopy(named().get(entry_of or name))
        async with Fake() as fake:
            replayed = await replay(case, fake)
        if entry_of is None:
            self.assertEqual(judged(case, stated, replayed)[1], "")
        for change, changed in ((wrong, replayed), (in_case, case), (in_entry, stated)):
            if change is not None:
                change(changed)
        said = judged(case, stated, replayed)[1]
        self.assertIn(word, said, name)

    async def test_a_request_that_is_not_python_s(self) -> None:
        def another_body(replayed):
            replayed.seen[0].body = b"{}"

        def another_key(replayed):
            replayed.seen[0].headers["authorization"] = "Bearer another"

        def a_header_more(replayed):
            replayed.seen[0].lines.append(("x-api-key", "placeholder-not-a-key"))

        def a_header_twice(replayed):
            replayed.seen[0].lines.append(("accept", "application/json"))

        def another_agent(replayed):
            replayed.seen[0].headers["user-agent"] += " (and more)"

        def no_agent(replayed):
            del replayed.seen[0].headers["user-agent"]

        def another_path(replayed):
            replayed.seen[0].target = "/v1/chat"

        def another_method(replayed):
            replayed.seen[0].method = "PUT"

        def no_request(replayed):
            replayed.seen.clear()

        def served_in_one_read(replayed):
            replayed.seen[0].reads = 1

        await self.refuses("plain_answer", "its body", another_body)
        await self.refuses("plain_answer", "authorization", another_key)
        await self.refuses("plain_answer", "which Python does not send", a_header_more)
        await self.refuses("plain_answer", "accept is sent 2 times", a_header_twice)
        await self.refuses("plain_answer", "user-agent is 'roomkit-providers/", another_agent)
        await self.refuses("plain_answer", "user-agent is None", no_agent)
        await self.refuses("plain_answer", "request 0: POST /v1/chat", another_path)
        await self.refuses("plain_answer", "request 0: PUT /v1/chat/completions", another_method)
        await self.refuses("plain_answer", "0 requests sent", no_request)
        await self.refuses("stream", "served in 1 reads", served_in_one_read)

    async def test_a_header_python_leaves_out_and_the_provider_sends(self) -> None:
        cases = {case["name"]: case for case in fixture()["cases"]}
        left_out = [
            name
            for name, value in cases["list_models"]["exchanges"][0]["request"]["headers"].items()
            if value is None
        ]
        self.assertNotEqual(left_out, [])

        def sent_all_the_same(replayed):
            replayed.seen[0].headers[left_out[0]] = "application/json"

        await self.refuses("list_models", f"request 0: {left_out[0]} is", sent_all_the_same)

    async def test_waits_that_are_not_python_s(self) -> None:
        def a_wait_less(replayed):
            replayed.waits.clear()

        def a_wait_more(replayed):
            replayed.waits.append(replayed.waits[0])

        def a_longer_wait(replayed):
            replayed.waits[0] = 0.51

        def a_backoff_too_short(replayed):
            replayed.waits[0] = 0.3

        def a_wait_a_little_short(replayed):
            replayed.waits[0] -= 0.01

        await self.refuses("retry_that_succeeds", "0 waits", a_wait_less)
        await self.refuses("retry_that_succeeds", "2 waits", a_wait_more)
        await self.refuses("retry_that_succeeds", "a wait of", a_longer_wait)
        await self.refuses("retry_that_succeeds", "a wait of", a_backoff_too_short)
        await self.refuses(
            "retry_after_the_wait_asked_as_a_date", "a wait of", a_wait_a_little_short
        )

    async def test_an_outcome_that_is_not_python_s(self) -> None:
        def another_text(replayed):
            replayed.given["content"] += "!"

        def another_vision(replayed):
            replayed.vision = not replayed.vision

        def an_event_less(replayed):
            replayed.given.pop()

        def two_ids_for_one_call(replayed):
            for event in replayed.given:
                if event["type"] == "tool_call":
                    event["id"] = "call_lookup_order_0123456789ab"

        def an_id_of_another_shape(replayed):
            for event in replayed.given:
                if "id" in event:
                    event["id"] = "call_lookup_order_0123"

        def an_id_of_another_name(replayed):
            for event in replayed.given:
                if "id" in event:
                    event["id"] = "call_refund_order_0123456789ab"

        def an_answer_all_the_same(replayed):
            replayed.given = {"content": "Yes."}

        def a_failure(replayed):
            replayed.error = ProviderError("refused")

        def no_failure(replayed):
            replayed.error = None

        def nothing_asked(replayed):
            replayed.asked = False

        def not_built(replayed):
            replayed.unbuilt = ProviderError("the provider cannot be built")

        await self.refuses("plain_answer", "the response", another_text)
        await self.refuses("plain_answer", "supports_vision", another_vision)
        await self.refuses("stream", "the events", an_event_less)
        await self.refuses("stream_cut_before_its_end", "the events", an_event_less)
        await self.refuses("stream_tool_call_without_an_id", "the events", two_ids_for_one_call)
        await self.refuses("stream_tool_call_without_an_id", "the events", an_id_of_another_shape)
        await self.refuses("stream_tool_call_without_an_id", "the events", an_id_of_another_name)
        await self.refuses(
            "answer_200_that_is_a_page", "where Python gives none", an_answer_all_the_same
        )
        await self.refuses("plain_answer", "a failure where Python answers", a_failure)
        await self.refuses("answer_200_that_is_a_page", "an answer where Python fails", no_failure)
        await self.refuses("plain_answer", "nothing was asked", nothing_asked)
        await self.refuses("plain_answer", "the provider is not built", not_built)

    async def test_a_failure_that_is_not_python_s(self) -> None:
        def another_status(replayed):
            replayed.error.status_code = 500

        def other_words(replayed):
            replayed.error.args = (replayed.error.args[0] + "!",)

        def not_worth_a_retry(replayed):
            replayed.error.retryable = not replayed.error.retryable

        def another_provider(replayed):
            replayed.error.provider = "another"

        def no_overflow(replayed):
            replayed.error.context_overflow = None

        def after_the_request(replayed):
            replayed.error.before_request = False

        def another_class(replayed):
            replayed.schema = False

        def another_reason(replayed):
            replayed.error.reason = "truncated"

        def no_rule(case):
            del case["compare"]

        def nothing_compared(case):
            case["compare"]["exactly"] = []

        object_529 = "answer_200_that_is_an_error_object"
        await self.refuses(object_529, "status_code differs", another_status)
        await self.refuses(object_529, "the failure says", other_words)
        await self.refuses(object_529, "retryable differs", not_worth_a_retry)
        await self.refuses(object_529, "provider differs", another_provider)
        await self.refuses("context_overflow_by_its_code", "context_overflow differs", no_overflow)
        await self.refuses("tool_name_openai_refuses", "before_request differs", after_the_request)
        await self.refuses("response_schema_refusal", "class differs", another_class)
        await self.refuses("response_schema_refusal", "reason differs", another_reason)
        await self.refuses(object_529, "no rule to compare it", in_case=no_rule)
        await self.refuses(object_529, "no provider reproduces", in_case=nothing_compared)

    async def test_a_named_case_that_is_not_as_its_entry_states(self) -> None:
        otherwise = "it differs from Python otherwise than its entry states"

        def another_text(replayed):
            replayed.given["content"] += "!"

        def another_finish(replayed):
            replayed.given["finish_reason"] = "length"

        def another_usage(replayed):
            replayed.given["usage"] = {"input_tokens": 1}

        def a_header_more(replayed):
            replayed.seen[0].lines.append(("x-api-key", "placeholder-not-a-key"))

        def retried(replayed):
            replayed.error.retryable = True

        def another_status(replayed):
            replayed.error.status_code = 308

        def an_overflow(replayed):
            replayed.error.context_overflow = True

        def before_the_request(replayed):
            replayed.error.before_request = True

        def a_request_more(replayed):
            replayed.seen.append(replayed.seen[0])

        def other_words(replayed):
            error = replayed.unbuilt or replayed.error
            error.args = ("other words",)

        def a_word_more(replayed):
            replayed.error.args = (replayed.error.args[0] + "!",)

        def after_the_request(replayed):
            replayed.unbuilt.before_request = False

        def worth_a_retry(replayed):
            replayed.unbuilt.retryable = True

        def a_request_more_stated(stated):
            stated["requests"] += 1

        await self.refuses("usage_counts_below_zero", "not {", another_text)
        await self.refuses("usage_counts_below_zero", "not {", another_finish)
        await self.refuses("usage_counts_below_zero", "not {", another_usage)
        await self.refuses("usage_counts_below_zero", otherwise, a_header_more)
        await self.refuses("redirect", "not {", retried)
        await self.refuses("redirect", "not {", another_status)
        await self.refuses("redirect", "not {", an_overflow)
        await self.refuses("redirect", "not {", before_the_request)
        await self.refuses("redirect", "not {", other_words)
        await self.refuses("redirect", "not {", a_word_more)
        await self.refuses("redirect", otherwise, a_request_more)
        await self.refuses("redirect", "not {", in_entry=a_request_more_stated)
        await self.refuses("stream_call_index_that_is_a_text", "not {", other_words)
        unbuilt = "base_url_with_a_trailing_slash_and_a_query"
        await self.refuses(unbuilt, "not {", other_words)
        await self.refuses(unbuilt, "not {", after_the_request)
        await self.refuses(unbuilt, "not {", worth_a_retry)
        # A case that replays as Python, held to the entry of a named one.
        await self.refuses("plain_answer", "named, and it replays as Python", entry_of="redirect")

    async def test_a_named_stream_that_is_not_as_its_entry_states(self) -> None:
        def another_text(replayed):
            for event in replayed.given:
                if event["type"] == "text_delta":
                    event["text"] += "!"

        def another_finish(replayed):
            replayed.given[-1]["finish_reason"] = "length"

        def another_usage(replayed):
            replayed.given[-1]["usage"] = {"input_tokens": 1}

        def no_end(replayed):
            replayed.given.pop()

        named_stream = "stream_usage_completion_tokens_as_text"
        await self.refuses(named_stream, "not {", another_text)
        await self.refuses(named_stream, "not {", another_finish)
        await self.refuses(named_stream, "not {", another_usage)
        await self.refuses(named_stream, "not {", no_end)

    async def test_a_case_asked_without_a_key_that_is_not_as_its_entry_states(self) -> None:
        def a_key(replayed):
            replayed.seen[0].headers["authorization"] = "Bearer another"

        def another_status(replayed):
            replayed.error.status_code = 500

        await self.refuses("empty_key", "not {", a_key)
        await self.refuses("empty_key", "not {", another_status)


if __name__ == "__main__":
    unittest.main()
