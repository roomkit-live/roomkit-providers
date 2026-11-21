"""The recorded wire cases, replayed through the RoomKit adapter.

Each case of ``python_chat_cases.json`` is asked as RoomKit asks it: its
configuration is RoomKit's ``OpenAIConfig``, its request an ``AIContext``,
and what the adapter gives are RoomKit's models, written back in the JSON
form the fixture holds; a failure is RoomKit's own exception. The outcome of
each case is what the package's own replay expects of it
(``tests/python/test_replay.py``): Python's for a case held as Python, its
entry's for a named one.
"""

from __future__ import annotations

import unittest
from collections import Counter
from typing import Any

import adapter_tests  # noqa: F401
from fake_endpoint import Fake
from replay_cases import Through, fixture, judged, named, replay
from roomkit.providers.ai import AIContext, ProviderError, ResponseSchemaError
from roomkit.providers.openai import OpenAIConfig
from roomkit_providers.roomkit import OpenAIAIProvider
from test_replay import LEFT_TO_RUST, NEVER_ANSWERED, SHORT_TIMEOUT


def written(call: str, given: Any) -> Any:
    """What the adapter gave, RoomKit's models, in the fixture's JSON form:
    a response without the tasks and observations RoomKit adds."""
    if call == "generate":
        return given.model_dump(mode="json", exclude={"tasks", "observations"})
    if call == "generate_stream":
        return given
    return [item.model_dump(mode="json") for item in given]


ADAPTER = Through(
    provider=lambda config: OpenAIAIProvider(OpenAIConfig(**config)),
    request=AIContext.model_validate,
    written=written,
    failure=ProviderError,
    schema_failure=ResponseSchemaError,
)


class AdapterReplayTests(unittest.IsolatedAsyncioTestCase):
    async def test_every_case_replays_through_the_adapter_as_through_the_package(self) -> None:
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
                replayed = await replay(case, fake, read_timeout=short, through=ADAPTER)
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

    async def test_the_replay_refuses_what_is_not_as_stated(self) -> None:
        """The comparison bites on RoomKit's models and exceptions too: one
        thing of what was replayed is made wrong, and the replay says so."""
        cases = {case["name"]: case for case in fixture()["cases"]}

        def another_text(replayed):
            replayed.given["content"] += "!"

        def an_event_less(replayed):
            replayed.given.pop()

        def another_status(replayed):
            replayed.error.status_code = 500

        def no_schema_failure(replayed):
            replayed.schema = False

        wrongs = [
            ("plain_answer", "the response", another_text),
            ("stream", "the events", an_event_less),
            ("answer_200_that_is_an_error_object", "status_code", another_status),
            ("response_schema_refusal", "class", no_schema_failure),
            ("redirect", "not {", another_status),
        ]
        async with Fake() as fake:
            for name, word, wrong in wrongs:
                with self.subTest(name=name):
                    replayed = await replay(cases[name], fake, through=ADAPTER)
                    stated = named().get(name)
                    self.assertEqual(judged(cases[name], stated, replayed)[1], "")
                    wrong(replayed)
                    self.assertIn(word, judged(cases[name], stated, replayed)[1])


if __name__ == "__main__":
    unittest.main()
