"""The recorded wire cases, replayed through the Python binding.

Each case of ``python_chat_cases.json`` is asked of the provider through the
package, against a local fake of the endpoint that serves each exchange, a
stream read by read at the recorded bytes. What the provider sent and gave
is what the Rust provider sends and gives: for a case the Rust replay holds
as Python, what Python did; for a case it names, what its entry states.

Left to the Rust replay, which moves its own clock: the cases whose answer
stops under way, and the timeouts each request is given, which no server
sees.
"""

from __future__ import annotations

import unittest
from collections import Counter

from fake_endpoint import Fake
from replay_cases import fixture, goes_silent, judged, named, replay

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
                ("generate", "as Python"): 183,
                ("generate", "named"): 24,
                ("generate", "left to Rust"): 2,
                ("generate_structured_stream", "as Python"): 93,
                ("generate_structured_stream", "named"): 17,
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
        self.assertEqual(len(named()), 46)


class ReplayRefusesTests(unittest.IsolatedAsyncioTestCase):
    """The replay held to what it compares: a case is replayed, found to be
    as stated, then one thing of what was replayed is made wrong, and the
    replay must say so."""

    async def refuses(self, name: str, word: str, wrong) -> None:
        cases = {case["name"]: case for case in fixture()["cases"]}
        stated = named().get(name)
        async with Fake() as fake:
            replayed = await replay(cases[name], fake)
        self.assertEqual(judged(cases[name], stated, replayed)[1], "")
        wrong(replayed)
        said = judged(cases[name], stated, replayed)[1]
        self.assertIn(word, said, name)

    async def test_a_request_that_is_not_python_s(self) -> None:
        def another_body(replayed):
            replayed.seen[0].body = b"{}"

        def another_key(replayed):
            replayed.seen[0].headers["authorization"] = "Bearer another"

        def a_header_more(replayed):
            replayed.seen[0].lines.append(("x-api-key", "placeholder-not-a-key"))

        def another_path(replayed):
            replayed.seen[0].target = "/v1/chat"

        def no_request(replayed):
            replayed.seen.clear()

        def served_in_one_read(replayed):
            replayed.seen[0].reads = 1

        await self.refuses("plain_answer", "its body", another_body)
        await self.refuses("plain_answer", "authorization", another_key)
        await self.refuses("plain_answer", "which Python does not send", a_header_more)
        await self.refuses("plain_answer", "request 0: POST /v1/chat", another_path)
        await self.refuses("plain_answer", "0 requests sent", no_request)
        await self.refuses("stream", "served in 1 reads", served_in_one_read)

    async def test_waits_that_are_not_python_s(self) -> None:
        def a_wait_less(replayed):
            replayed.waits.clear()

        def a_longer_wait(replayed):
            replayed.waits[0] = 0.51

        def a_wait_a_little_short(replayed):
            replayed.waits[0] -= 0.01

        await self.refuses("retry_that_succeeds", "0 waits", a_wait_less)
        await self.refuses("retry_that_succeeds", "a wait of", a_longer_wait)
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

        def another_status(replayed):
            replayed.error.status_code = 500

        def other_words(replayed):
            replayed.error.message += "!"

        def two_ids_for_one_call(replayed):
            for event in replayed.given:
                if event["type"] == "tool_call":
                    event["id"] = "call_lookup_order_0123456789ab"

        await self.refuses("plain_answer", "the response", another_text)
        await self.refuses("plain_answer", "supports_vision", another_vision)
        await self.refuses("stream", "the events", an_event_less)
        await self.refuses("stream_cut_before_its_end", "the events", an_event_less)
        await self.refuses("answer_200_that_is_an_error_object", "status_code", another_status)
        await self.refuses("answer_200_that_is_an_error_object", "the failure says", other_words)
        await self.refuses("stream_tool_call_without_an_id", "the events", two_ids_for_one_call)

    async def test_a_named_case_that_is_not_as_its_entry_states(self) -> None:
        otherwise = "it differs from Python otherwise than its entry states"

        def another_text(replayed):
            replayed.given["content"] += "!"

        def a_header_more(replayed):
            replayed.seen[0].lines.append(("x-api-key", "placeholder-not-a-key"))

        def retried(replayed):
            replayed.error.retryable = True

        def a_request_more(replayed):
            replayed.seen.append(replayed.seen[0])

        def other_words(replayed):
            replayed.unbuilt.message += "!"

        await self.refuses("usage_counts_below_zero", "not {", another_text)
        await self.refuses("usage_counts_below_zero", otherwise, a_header_more)
        await self.refuses("redirect", "not {", retried)
        await self.refuses("redirect", otherwise, a_request_more)
        await self.refuses("base_url_with_a_trailing_slash_and_a_query", "not {", other_words)


if __name__ == "__main__":
    unittest.main()
