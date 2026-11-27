"""The cases ``generate_openai_chat_cases.py`` runs on the RoomKit Python
providers of the vendors that speak OpenAI's Chat Completions with rules of
their own: DeepSeek, Kimi and MiniMax, each a derivative of
``OpenAIAIProvider``.

A vendor's cases show what its class redefines, on its own endpoint and
behind a base URL: its name, the tool names it takes, how a turn's sampling
and reasoning are sent, where a round's earlier reasoning goes back, how it
reports usage, which models it knows, and what it answers a failure with.
What it inherits as it is (the wire, its streams, its retries) is OpenAI's,
whose cases show it.
"""

from __future__ import annotations

from typing import Any

from openai_chat_cases import (
    CITY,
    HELLO,
    LOOKUP,
    ROUND,
    TOOLS,
    completion,
    failed,
    listed,
    said,
)
from wire_capture import (
    GENERATE,
    KEY,
    LISTING,
    STREAM,
    Case,
    answer,
    registrar,
    stream,
)

PROXY = "https://llm.example/v1"

# A tool whose name holds a dot, which neither DeepSeek nor Kimi takes on its
# own endpoint.
DOTTED = {**LOOKUP, "name": "orders.lookup"}
DOTTED_TOOLS = {**HELLO, "tools": [DOTTED]}

# A round that called a tool without reasoning, before the turn's own.
ROUND_WITHOUT_REASONING = [
    {"role": "user", "content": "Where is order A-1?"},
    {
        "role": "assistant",
        "content": [
            {"type": "text", "text": "Let me look."},
            {
                "type": "tool_call",
                "id": "call_1",
                "name": "lookup_order",
                "arguments": {"number": "A-1"},
            },
        ],
    },
    {
        "role": "tool",
        "content": [
            {
                "type": "tool_result",
                "tool_call_id": "call_1",
                "name": "lookup_order",
                "result": "Shipped.",
            }
        ],
    },
]


def asked(**members: Any) -> dict[str, Any]:
    """``HELLO``, with ``members`` of the request."""
    return {**HELLO, **members}


def refusing(case: Any, refused: frozenset[str]) -> Any:
    """``case``, adding the cases named in ``refused`` with no reply: the
    vendor's provider refuses each before it sends a request."""

    def added(
        name: str,
        kinds: str,
        call: str,
        config: dict[str, Any],
        request: dict[str, Any],
        *replies: Any,
        **more: Any,
    ) -> None:
        case(name, kinds, call, config, request, *(() if name in refused else replies), **more)

    return added


def common(case: Any, config: dict[str, Any], proxy: dict[str, Any]) -> None:
    """The cases every vendor has, on ``config`` (its own endpoint) and
    ``proxy`` (a server of one's own)."""
    case("plain_answer", "plain", GENERATE, config, HELLO, answer(completion()))
    case("plain_stream", "plain stream", STREAM, config, HELLO, stream(said("Hello", " there.")))
    case(
        "temperature_of_the_request",
        "sampling",
        GENERATE,
        config,
        asked(temperature=0.2),
        answer(completion()),
    )
    case(
        "temperature_stated_by_the_configuration",
        "sampling",
        GENERATE,
        {**config, "supports_custom_temperature": True},
        asked(temperature=0.2),
        answer(completion()),
    )
    case(
        "token_bound_of_the_request",
        "sampling",
        GENERATE,
        config,
        asked(max_tokens=256),
        answer(completion()),
    )
    case(
        "token_bound_as_max_tokens",
        "sampling",
        GENERATE,
        {**config, "use_max_completion_tokens": False, "max_tokens": 512},
        HELLO,
        answer(completion()),
    )
    for effort in ("minimal", "low", "medium", "high", "xhigh", "max", "none"):
        case(
            f"reasoning_effort_{effort}",
            "reasoning",
            GENERATE,
            config,
            asked(reasoning_effort=effort),
            answer(completion()),
        )
        case(
            f"reasoning_effort_{effort}_behind_a_base_url",
            "reasoning proxy",
            GENERATE,
            proxy,
            asked(reasoning_effort=effort),
            answer(completion()),
        )
    case(
        "reasoning_effort_of_the_configuration",
        "reasoning",
        GENERATE,
        {**config, "reasoning_effort": "high"},
        HELLO,
        answer(completion()),
    )
    case(
        "reasoning_effort_of_the_request_over_the_configuration",
        "reasoning",
        GENERATE,
        {**config, "reasoning_effort": "high"},
        asked(reasoning_effort="low"),
        answer(completion()),
    )
    case(
        "thinking_turned_off",
        "reasoning",
        GENERATE,
        config,
        asked(enable_thinking=False),
        answer(completion()),
    )
    case(
        "thinking_turned_on",
        "reasoning",
        GENERATE,
        config,
        asked(enable_thinking=True),
        answer(completion()),
    )
    case(
        "thinking_budget_of_zero",
        "reasoning",
        GENERATE,
        config,
        asked(thinking_budget=0),
        answer(completion()),
    )
    case(
        "thinking_budget",
        "reasoning",
        GENERATE,
        config,
        asked(thinking_budget=2048),
        answer(completion()),
    )
    case(
        "reasoning_on_a_tool_turn",
        "reasoning tools",
        GENERATE,
        config,
        {**TOOLS, "reasoning_effort": "high"},
        answer(completion()),
    )
    case(
        "earlier_reasoning_on_a_tool_round",
        "round_reasoning tools",
        GENERATE,
        config,
        {**TOOLS, "messages": ROUND},
        answer(completion()),
    )
    case(
        "earlier_round_without_reasoning",
        "round_reasoning tools",
        GENERATE,
        config,
        {**TOOLS, "messages": ROUND_WITHOUT_REASONING},
        answer(completion()),
    )
    case(
        "earlier_reasoning_behind_a_base_url",
        "round_reasoning tools proxy",
        GENERATE,
        proxy,
        {**TOOLS, "messages": ROUND},
        answer(completion()),
    )
    case(
        "reasoning_read_from_the_answer",
        "round_reasoning",
        GENERATE,
        config,
        HELLO,
        answer(completion("Hello.", reasoning_content="The user greets me.")),
    )
    case("dotted_tool_name", "tool_names", GENERATE, config, DOTTED_TOOLS, answer(completion()))
    case(
        "dotted_tool_name_behind_a_base_url",
        "tool_names proxy",
        GENERATE,
        proxy,
        DOTTED_TOOLS,
        answer(completion()),
    )
    case(
        "response_schema",
        "response_schema",
        GENERATE,
        config,
        asked(response_schema=CITY),
        answer(completion('{"city": "Paris"}')),
    )
    case(
        "usage_with_cached_tokens",
        "usage",
        GENERATE,
        config,
        HELLO,
        answer(
            completion(
                usage={
                    "prompt_tokens": 30,
                    "completion_tokens": 5,
                    "total_tokens": 35,
                    "prompt_tokens_details": {"cached_tokens": 20},
                }
            )
        ),
    )
    case(
        "usage_with_reasoning_tokens",
        "usage",
        GENERATE,
        config,
        HELLO,
        answer(
            completion(
                usage={
                    "prompt_tokens": 12,
                    "completion_tokens": 40,
                    "total_tokens": 52,
                    "completion_tokens_details": {"reasoning_tokens": 30},
                }
            )
        ),
    )
    case(
        "status_400",
        "failure",
        GENERATE,
        config,
        HELLO,
        answer(failed("Invalid request.", "invalid_request_error"), status=400),
    )
    case(
        "status_401",
        "failure",
        GENERATE,
        config,
        HELLO,
        answer(failed("Authentication failed.", "authentication_error"), status=401),
    )
    case(
        "models_listed",
        "listing",
        LISTING,
        config,
        HELLO,
        answer(listed(config["model"])),
    )
    case(
        "a_model_the_catalogue_does_not_name",
        "catalogue",
        GENERATE,
        {**config, "model": "vendor-next"},
        HELLO,
        answer(completion()),
    )
    case(
        "a_model_behind_a_base_url", "catalogue proxy", GENERATE, proxy, HELLO, answer(completion())
    )


DEEPSEEK_CASES: list[Case] = []
_deepseek = refusing(
    registrar(DEEPSEEK_CASES),
    frozenset(
        {
            "dotted_tool_name",
            "response_schema",
            "response_schema_behind_a_base_url",
            "deepseek_s_other_base",
        }
    ),
)
DEEPSEEK = {"api_key": KEY, "model": "deepseek-v4-pro"}
common(_deepseek, DEEPSEEK, {**DEEPSEEK, "base_url": PROXY})
for _enabled in (True, False):
    _deepseek(
        f"thinking_of_the_configuration_{str(_enabled).lower()}",
        "reasoning",
        GENERATE,
        {**DEEPSEEK, "enable_thinking": _enabled},
        HELLO,
        answer(completion()),
    )
_deepseek(
    "thinking_of_the_request_over_the_configuration",
    "reasoning",
    GENERATE,
    {**DEEPSEEK, "enable_thinking": True},
    asked(enable_thinking=False),
    answer(completion()),
)
_deepseek(
    "usage_with_deepseek_s_cache_counters",
    "usage",
    GENERATE,
    DEEPSEEK,
    HELLO,
    answer(
        completion(
            usage={
                "prompt_tokens": 30,
                "completion_tokens": 5,
                "total_tokens": 35,
                "prompt_cache_hit_tokens": 20,
                "prompt_cache_miss_tokens": 10,
            }
        )
    ),
)
_deepseek(
    "usage_with_a_cache_hit_and_no_miss",
    "usage",
    GENERATE,
    DEEPSEEK,
    HELLO,
    answer(
        completion(
            usage={
                "prompt_tokens": 30,
                "completion_tokens": 5,
                "total_tokens": 35,
                "prompt_cache_hit_tokens": 20,
            }
        )
    ),
)
_deepseek(
    "usage_with_no_cache_hit",
    "usage",
    GENERATE,
    DEEPSEEK,
    HELLO,
    answer(
        completion(
            usage={
                "prompt_tokens": 30,
                "completion_tokens": 5,
                "total_tokens": 35,
                "prompt_cache_hit_tokens": 0,
                "prompt_cache_miss_tokens": 30,
            }
        )
    ),
)
_deepseek(
    "response_schema_behind_a_base_url",
    "response_schema proxy",
    GENERATE,
    {**DEEPSEEK, "base_url": PROXY},
    asked(response_schema=CITY),
    answer(completion('{"city": "Paris"}')),
)
_deepseek(
    "response_schema_stated_by_the_configuration",
    "response_schema",
    GENERATE,
    {**DEEPSEEK, "supports_response_schema": True},
    asked(response_schema=CITY),
    answer(completion('{"city": "Paris"}')),
)
_deepseek(
    "deepseek_s_other_base",
    "endpoint",
    GENERATE,
    {**DEEPSEEK, "base_url": "https://api.deepseek.com"},
    DOTTED_TOOLS,
    answer(completion()),
)
_deepseek(
    "flash_model",
    "catalogue",
    GENERATE,
    {**DEEPSEEK, "model": "deepseek-v4-flash"},
    HELLO,
    answer(completion()),
)

KIMI_CASES: list[Case] = []
_kimi = refusing(registrar(KIMI_CASES), frozenset({"dotted_tool_name"}))
KIMI = {"api_key": KEY, "model": "kimi-k3.1-pro"}
common(_kimi, KIMI, {**KIMI, "base_url": PROXY, "model": "moonshotai/Kimi-K3"})
_kimi(
    "k3_model",
    "catalogue",
    GENERATE,
    {**KIMI, "model": "kimi-k3"},
    asked(reasoning_effort="medium"),
    answer(completion()),
)
_kimi(
    "highspeed_model",
    "catalogue",
    GENERATE,
    {**KIMI, "model": "kimi-k3.1-pro-highspeed"},
    HELLO,
    answer(completion()),
)
_kimi(
    "a_catalogued_model_behind_a_base_url",
    "catalogue proxy",
    GENERATE,
    {**KIMI, "base_url": PROXY},
    asked(reasoning_effort="none"),
    answer(completion()),
)
_kimi(
    "earlier_round_without_reasoning_streamed",
    "round_reasoning tools stream",
    STREAM,
    KIMI,
    {**TOOLS, "messages": ROUND_WITHOUT_REASONING},
    stream(said("Shipped.")),
)

MINIMAX_CASES: list[Case] = []
_minimax = refusing(registrar(MINIMAX_CASES), frozenset({"response_schema"}))
MINIMAX = {"api_key": KEY, "model": "MiniMax-M3"}
common(_minimax, MINIMAX, {**MINIMAX, "base_url": PROXY, "model": "minimax-m3-local"})
FLASH = {**MINIMAX, "model": "MiniMax-M3.1-Flash"}
for _effort in ("minimal", "low", "medium", "high", "max", "none"):
    _minimax(
        f"flash_reasoning_effort_{_effort}",
        "reasoning catalogue",
        GENERATE,
        FLASH,
        asked(reasoning_effort=_effort),
        answer(completion()),
    )
_minimax(
    "flash_thinking_turned_off",
    "reasoning catalogue",
    GENERATE,
    FLASH,
    asked(enable_thinking=False),
    answer(completion()),
)
_minimax(
    "m3_off_with_extra_body",
    "reasoning",
    GENERATE,
    {**MINIMAX, "extra_body": {"thinking": {"type": "enabled"}, "top_k": 40}},
    asked(reasoning_effort="none"),
    answer(completion()),
)

# The table of each vendor, by the name its fixture is written under.
VENDOR_CASES: dict[str, list[Case]] = {
    "deepseek": DEEPSEEK_CASES,
    "kimi": KIMI_CASES,
    "minimax": MINIMAX_CASES,
}
