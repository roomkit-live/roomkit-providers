"""The cases ``generate_openai_chat_cases.py`` runs on RoomKit's Python OpenAI
provider: for each, a configuration, a request, and what the fake endpoint
answers, in order.

A case names its ``kinds``: what it shows of the provider. The Rust test that
reads the fixture lists the kinds it requires, so a table that loses one, or
names one the test does not know, fails there.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any

KEY = "placeholder-not-a-key"
REQUEST_KEY = "placeholder-not-a-key-either"

JSON = {"content-type": "application/json"}
EVENTS = {"content-type": "text/event-stream"}

SDK_RETRIES = (
    "The openai SDK decides which answers it tries again, how many times and after what wait."
)
SDK_WORDS = "No answer came: the message is the openai SDK's."
PYTHON_WORDS = "The message holds what Python's JSON reader says of the text, in its words."


@dataclass(frozen=True)
class Reply:
    """What the fake endpoint answers one request with."""

    status: int = 200
    headers: dict[str, str] = field(default_factory=dict)
    body: Any = None
    text: str | None = None
    chunks: list[str] | None = None
    cut: bool = False
    failure: str | None = None


@dataclass(frozen=True)
class Case:
    """One run of the provider."""

    name: str
    kinds: frozenset[str]
    call: str
    config: dict[str, Any]
    request: dict[str, Any]
    replies: tuple[Reply, ...]
    note: str | None = None
    sdk: str | None = None


CASES: list[Case] = []

GENERATE = "generate"
TEXT = "generate_stream"
STREAM = "generate_structured_stream"


def case(
    name: str,
    kinds: str,
    call: str,
    config: dict[str, Any],
    request: dict[str, Any],
    *replies: Reply,
    note: str | None = None,
    sdk: str | None = None,
) -> None:
    CASES.append(Case(name, frozenset(kinds.split()), call, config, request, replies, note, sdk))


# -- What the fake answers ---------------------------------------------------

USAGE = {"prompt_tokens": 12, "completion_tokens": 5, "total_tokens": 17}
ANSWERED_BY = "gpt-4o-2024-08-06"


def answer(body: Any, status: int = 200, **headers: str) -> Reply:
    return Reply(status=status, headers={**JSON, **headers}, body=body)


def page(text: str, status: int, content_type: str = "text/html") -> Reply:
    return Reply(status=status, headers={"content-type": content_type}, text=text)


def stream(chunks: list[str], *, cut: bool = False, status: int = 200) -> Reply:
    return Reply(status=status, headers=dict(EVENTS), chunks=chunks, cut=cut)


def completion(
    content: str | None = "Hello there.",
    *,
    finish: str | None = "stop",
    model: str | None = ANSWERED_BY,
    usage: dict[str, Any] | None = USAGE,
    tool_calls: list[dict[str, Any]] | None = None,
    **message: Any,
) -> dict[str, Any]:
    said: dict[str, Any] = {"role": "assistant", "content": content, **message}
    if tool_calls is not None:
        said["tool_calls"] = tool_calls
    body: dict[str, Any] = {"id": "chatcmpl-1", "object": "chat.completion", "created": 1700000000}
    if model is not None:
        body["model"] = model
    body["choices"] = [{"index": 0, "message": said, "finish_reason": finish}]
    if usage is not None:
        body["usage"] = usage
    return body


def called(call_id: str | None, name: str, arguments: str) -> dict[str, Any]:
    call: dict[str, Any] = {"type": "function", "function": {"name": name, "arguments": arguments}}
    return call if call_id is None else {"id": call_id, **call}


def chunk(
    delta: dict[str, Any] | None = None,
    *,
    finish: str | None = None,
    usage: dict[str, Any] | None = None,
    model: str = ANSWERED_BY,
) -> dict[str, Any]:
    choices = [] if delta is None else [{"index": 0, "delta": delta, "finish_reason": finish}]
    body: dict[str, Any] = {
        "id": "chatcmpl-1",
        "object": "chat.completion.chunk",
        "created": 1700000000,
        "model": model,
        "choices": choices,
    }
    if usage is not None:
        body["usage"] = usage
    return body


def sse(*events: Any, done: bool = True) -> list[str]:
    """Each event as one read of the stream, then the end mark."""
    lines = [f"data: {json.dumps(event, separators=(',', ':'))}\n\n" for event in events]
    return [*lines, "data: [DONE]\n\n"] if done else lines


def said(*texts: str, finish: str = "stop", usage: dict[str, Any] | None = USAGE) -> list[str]:
    """A streamed answer: the role, each text, the end, then the usage."""
    events = [chunk({"role": "assistant", "content": ""})]
    events += [chunk({"content": text}) for text in texts]
    events.append(chunk({}, finish=finish))
    if usage is not None:
        events.append(chunk(usage=usage))
    return sse(*events)


def fragment(index: int, call_id: str | None, name: str | None, arguments: str) -> dict[str, Any]:
    """A streamed fragment of a tool call."""
    function = {"arguments": arguments} if name is None else {"name": name, "arguments": arguments}
    call: dict[str, Any] = {"index": index, "function": function}
    if call_id is not None:
        call = {"index": index, "id": call_id, "type": "function", "function": function}
    return chunk({"tool_calls": [call]})


def cut_every(chunks: list[str], size: int) -> list[str]:
    """The same bytes, read ``size`` characters at a time."""
    whole = "".join(chunks)
    return [whole[at : at + size] for at in range(0, len(whole), size)]


def failed(message: str, kind: str, code: str | None = None, param: str | None = None) -> Any:
    return {"error": {"message": message, "type": kind, "param": param, "code": code}}


REFUSED = Reply(failure="connection refused")
TIMED_OUT = Reply(failure="timeout")

# -- What a case is given ------------------------------------------------------

OPENAI = {"api_key": KEY, "model": "gpt-4o"}
LOCAL = {"api_key": KEY, "model": "local-model", "base_url": "https://llm.example/v1"}
HELLO = {"messages": [{"role": "user", "content": "Hello"}]}
PNG = "data:image/png;base64,iVBORw0KGgo="
LOOKUP = {
    "name": "lookup_order",
    "description": "Find an order by its number",
    "parameters": {
        "type": "object",
        "properties": {"number": {"type": "string"}},
        "required": ["number"],
    },
}
CLOCK = {"name": "clock", "description": "Tell the time"}
TOOLS = {**HELLO, "tools": [LOOKUP, CLOCK]}
CITY = {
    "type": "object",
    "properties": {"city": {"type": "string"}},
    "required": ["city"],
    "additionalProperties": False,
}
ASKED_A_CITY = {**HELLO, "response_schema": CITY}
PARIS = '{"city": "Paris"}'
LOOKED_UP = called("call_1", "lookup_order", '{"number": "A-1"}')
ROUND = [
    {"role": "user", "content": "Where is order A-1?"},
    {
        "role": "assistant",
        "content": [
            {"type": "thinking", "thinking": "The order number is A-1."},
            {"type": "text", "text": "Let me look."},
            {
                "type": "tool_call",
                "id": "call_1",
                "name": "lookup_order",
                "arguments": {"number": "A-1"},
            },
            {"type": "tool_call", "id": "call_2", "name": "clock", "arguments": {}},
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
            },
            {"type": "tool_result", "tool_call_id": "call_2", "name": "clock", "result": "12:00"},
        ],
    },
]

# -- A request and its answer ---------------------------------------------------

case("plain_answer", "plain finish_stop", GENERATE, OPENAI, HELLO, answer(completion()))
case(
    "system_prompt",
    "system_prompt",
    GENERATE,
    OPENAI,
    {**HELLO, "system_prompt": "You answer in one sentence."},
    answer(completion()),
)
case(
    "several_turns",
    "turns",
    GENERATE,
    OPENAI,
    {
        "messages": [
            {"role": "system", "content": "The customer writes from the web."},
            {"role": "user", "content": "Hello"},
            {"role": "assistant", "content": "Hello. How can I help?"},
            {"role": "user", "content": [{"type": "text", "text": "Where is my order?"}]},
            {"role": "assistant", "content": ""},
            {"role": "user", "content": []},
        ]
    },
    answer(completion("Which order?")),
)
case(
    "temperature_and_token_bound_of_the_request",
    "plain",
    GENERATE,
    OPENAI,
    {**HELLO, "temperature": 0.2, "max_tokens": 256},
    answer(completion()),
)
case(
    "no_token_bound",
    "plain",
    GENERATE,
    {**OPENAI, "max_tokens": None},
    HELLO,
    answer(completion()),
)
case(
    "the_answer_names_no_model",
    "plain",
    GENERATE,
    OPENAI,
    HELLO,
    answer(completion(model=None)),
    note="The response's model is the one asked when the answer names none.",
)

# -- Images -------------------------------------------------------------------------

case(
    "text_and_images",
    "image_data_uri image_url",
    GENERATE,
    OPENAI,
    {
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "What do these show?"},
                    {"type": "image", "url": PNG, "mime_type": "image/png"},
                    {"type": "image", "url": "https://example.com/label.png"},
                ],
            }
        ]
    },
    answer(completion("Two labels.")),
)
case(
    "image_whose_part_names_another_type",
    "image_data_uri",
    GENERATE,
    OPENAI,
    {
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "image",
                        "url": "data:;base64,iVBORw0K\nGgo",
                        "mime_type": "image/jpeg",
                    }
                ],
            }
        ]
    },
    answer(completion("A label.")),
    note=(
        "A data URI is rebuilt: the part's media type, and the payload without its line "
        "breaks and padded."
    ),
)
case(
    "image_that_is_no_base64",
    "image_data_uri before_request",
    GENERATE,
    OPENAI,
    {
        "messages": [
            {"role": "user", "content": [{"type": "image", "url": "data:image/png;base64,***"}]}
        ]
    },
    note="Refused before any request.",
)

# -- Tools ------------------------------------------------------------------------------

case("tools_declared", "tools", GENERATE, OPENAI, TOOLS, answer(completion()))
case(
    "tool_call",
    "tools tool_call finish_tool_calls",
    GENERATE,
    OPENAI,
    TOOLS,
    answer(completion(None, finish="tool_calls", tool_calls=[LOOKED_UP])),
)
case(
    "tool_calls_beside_text",
    "tool_call tool_calls_several finish_tool_calls",
    GENERATE,
    OPENAI,
    TOOLS,
    answer(
        completion(
            "Let me look.",
            finish="tool_calls",
            tool_calls=[LOOKED_UP, called("call_2", "clock", "{}"), called("call_3", "clock", "")],
        )
    ),
)
case(
    "tool_call_with_arguments_that_do_not_read",
    "tool_call tool_call_unreadable",
    GENERATE,
    OPENAI,
    TOOLS,
    answer(
        completion(
            None,
            finish="tool_calls",
            tool_calls=[
                called("call_1", "lookup_order", "number A-1"),
                called("call_2", "lookup_order", '["A-1"]'),
            ],
        )
    ),
)
case(
    "tool_call_cut_by_the_token_bound",
    "tool_call tool_call_unreadable finish_length",
    GENERATE,
    OPENAI,
    TOOLS,
    answer(
        completion(
            None,
            finish="length",
            tool_calls=[LOOKED_UP, called("call_2", "lookup_order", '{"number": "A-')],
        )
    ),
)
case(
    "tool_calls_without_an_id_of_their_own",
    "tool_call minted_call_id",
    GENERATE,
    OPENAI,
    TOOLS,
    answer(
        completion(
            None,
            finish="tool_calls",
            tool_calls=[
                called(None, "lookup_order", '{"number": "A-1"}'),
                called("call_7", "clock", "{}"),
                called("call_7", "clock", "{}"),
                called("", "", "{}"),
            ],
        )
    ),
)
case(
    "tool_call_entries_that_are_no_function",
    "tool_call",
    GENERATE,
    OPENAI,
    TOOLS,
    answer(
        completion(
            "Done.",
            finish="tool_calls",
            tool_calls=[{"id": "call_1", "type": "custom", "custom": {"name": "x", "input": "y"}}],
        )
    ),
)
case(
    "round_replayed_with_its_results",
    "tools tool_round_replayed",
    GENERATE,
    OPENAI,
    {"messages": ROUND, "tools": [LOOKUP, CLOCK]},
    answer(completion("It shipped at noon.")),
    note="The round's reasoning is not sent back.",
)
case(
    "tool_results_of_texts_and_images",
    "tool_round_replayed tool_result_parts image_data_uri",
    GENERATE,
    OPENAI,
    {
        "messages": [
            ROUND[0],
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_call",
                        "id": "call_1",
                        "name": "lookup_order",
                        "arguments": {"number": "A-1"},
                    },
                    {"type": "tool_call", "id": "call_2", "name": "clock", "arguments": {}},
                ],
            },
            {
                "role": "tool",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_call_id": "call_1",
                        "name": "lookup_order",
                        "result": [
                            {"type": "text", "text": "Shipped."},
                            {"type": "image", "url": PNG, "mime_type": "image/png"},
                            {"type": "text", "text": "Label above."},
                        ],
                    },
                    {
                        "type": "tool_result",
                        "tool_call_id": "call_2",
                        "name": "clock",
                        "result": [{"type": "image", "url": "https://example.com/clock.png"}],
                    },
                ],
            },
        ]
    },
    answer(completion("It shipped.")),
)
case(
    "tool_name_openai_refuses",
    "tools before_request",
    GENERATE,
    OPENAI,
    {**HELLO, "tools": [{"name": "orders.lookup", "description": "Find an order"}]},
    note="Refused before any request, on OpenAI's own endpoint.",
)
case(
    "tool_name_behind_a_base_url",
    "tools",
    GENERATE,
    LOCAL,
    {**HELLO, "tools": [{"name": "orders.lookup", "description": "Find an order"}]},
    answer(completion(model="local-model")),
)

# -- Reasoning, temperature and the token bound ----------------------------------

MODERN = {"api_key": KEY, "model": "gpt-5.1"}
case(
    "reasoning_effort_of_the_configuration",
    "reasoning_effort no_temperature max_completion_tokens",
    GENERATE,
    {**MODERN, "reasoning_effort": "high"},
    {**HELLO, "temperature": 0.2},
    answer(completion(model="gpt-5.1")),
)
case(
    "reasoning_effort_of_the_request",
    "reasoning_effort",
    GENERATE,
    {**MODERN, "reasoning_effort": "high"},
    {**HELLO, "reasoning_effort": "low"},
    answer(completion(model="gpt-5.1")),
)
case(
    "reasoning_turned_off_on_a_model_that_cannot",
    "reasoning_effort",
    GENERATE,
    {"api_key": KEY, "model": "gpt-5", "reasoning_effort": "high"},
    {**HELLO, "enable_thinking": False},
    answer(completion(model="gpt-5")),
    note="Off is the least effort the catalogue says the model takes.",
)
case(
    "reasoning_effort_none_on_a_model_that_cannot",
    "reasoning_effort",
    GENERATE,
    {"api_key": KEY, "model": "o3"},
    {**HELLO, "reasoning_effort": "none"},
    answer(completion(model="o3")),
)
case(
    "tools_on_a_model_that_takes_them_without_reasoning",
    "reasoning_effort tools",
    GENERATE,
    {"api_key": KEY, "model": "gpt-5.4", "reasoning_effort": "high"},
    TOOLS,
    answer(completion(model="gpt-5.4")),
    note="The catalogue tags the model: a turn with tools sends none.",
)
case(
    "tools_on_a_model_that_takes_them_without_reasoning_unset",
    "reasoning_effort tools",
    GENERATE,
    {"api_key": KEY, "model": "gpt-5.4"},
    TOOLS,
    answer(completion(model="gpt-5.4")),
)
case(
    "tools_on_a_model_that_keeps_its_effort",
    "reasoning_effort tools",
    GENERATE,
    {**MODERN, "reasoning_effort": "high"},
    TOOLS,
    answer(completion(model="gpt-5.1")),
)
case(
    "tools_and_effort_behind_a_base_url_unsaid",
    "reasoning_effort tools",
    GENERATE,
    {**LOCAL, "reasoning_effort": "high"},
    TOOLS,
    answer(completion(model="local-model")),
    note="Left out, and warned of once in Python's log.",
)
case(
    "tools_and_effort_behind_a_base_url_said_to_take_it",
    "reasoning_effort tools",
    GENERATE,
    {**LOCAL, "reasoning_effort": "high", "supports_reasoning_effort_with_tools": True},
    TOOLS,
    answer(completion(model="local-model")),
)
case(
    "tools_and_effort_behind_a_base_url_said_not_to",
    "reasoning_effort tools",
    GENERATE,
    {**LOCAL, "supports_reasoning_effort_with_tools": False},
    {**TOOLS, "reasoning_effort": "high"},
    answer(completion(model="local-model")),
)
case(
    "switches_stated_behind_a_base_url",
    "no_temperature max_completion_tokens",
    GENERATE,
    {**LOCAL, "use_max_completion_tokens": True, "supports_custom_temperature": False},
    {**HELLO, "temperature": 0.2},
    answer(completion(model="local-model")),
)
case(
    "modern_model_behind_a_base_url",
    "plain",
    GENERATE,
    {**LOCAL, "model": "gpt-5.1"},
    {**HELLO, "temperature": 0.2},
    answer(completion(model="gpt-5.1")),
    note="Behind a base URL nothing is known of the model: max_tokens and the temperature go.",
)
case(
    "model_the_endpoint_serves_on_responses_only",
    "before_request",
    GENERATE,
    {"api_key": KEY, "model": "gpt-5.5-pro"},
    HELLO,
    note="Refused before any request, from the catalogue.",
)
case(
    "model_that_takes_no_tools_on_chat_completions",
    "before_request tools",
    GENERATE,
    {"api_key": KEY, "model": "gpt-6-astra"},
    TOOLS,
    note="Refused before any request, from the catalogue.",
)

# -- Reasoning read, and sent back ----------------------------------------------

case(
    "think_tags",
    "thinking_read",
    GENERATE,
    LOCAL,
    HELLO,
    answer(
        completion(
            "\n<think>They greet me.</think>\nHello. <think>is text</think>", model="local-model"
        )
    ),
)
case(
    "think_block_never_closed",
    "thinking_read finish_length",
    GENERATE,
    LOCAL,
    HELLO,
    answer(completion("<think>They greet", finish="length", model="local-model")),
)
case(
    "reasoning_in_its_own_field",
    "thinking_read",
    GENERATE,
    LOCAL,
    HELLO,
    answer(completion("Hello.", model="local-model", reasoning_content="They greet me.")),
)
case(
    "reasoning_in_both_conventions",
    "thinking_read",
    GENERATE,
    LOCAL,
    HELLO,
    answer(completion("<think>First.</think>Hello.", model="local-model", reasoning=" Then.")),
)
THOUGHT = [
    ROUND[0],
    ROUND[1],
    ROUND[2],
    {
        "role": "assistant",
        "content": [
            {"type": "thinking", "thinking": "It shipped."},
            {"type": "text", "text": "It shipped at noon."},
        ],
    },
    {"role": "assistant", "content": [{"type": "thinking", "thinking": "Only a thought."}]},
    {"role": "assistant", "content": [{"type": "text", "text": "No thought."}]},
    {"role": "user", "content": "Thanks."},
]
case(
    "thinking_field_named_by_the_configuration",
    "thinking_field tool_round_replayed",
    GENERATE,
    {**LOCAL, "thinking_field": "reasoning_content"},
    {"messages": THOUGHT},
    answer(completion("You are welcome.", model="local-model")),
)
case(
    "thinking_not_sent_back_without_a_field",
    "thinking_field",
    GENERATE,
    LOCAL,
    {"messages": THOUGHT},
    answer(completion("You are welcome.", model="local-model")),
)

# -- A response schema ----------------------------------------------------------------

case(
    "response_schema", "response_schema", GENERATE, OPENAI, ASKED_A_CITY, answer(completion(PARIS))
)
case(
    "response_schema_after_a_think_block",
    "response_schema thinking_read",
    GENERATE,
    {**LOCAL, "supports_response_schema": True},
    ASKED_A_CITY,
    answer(completion(f"<think>A city.</think>{PARIS}", model="local-model")),
)
case(
    "response_schema_where_the_server_takes_none",
    "response_schema response_schema_unsupported before_request",
    GENERATE,
    {**LOCAL, "supports_response_schema": False},
    ASKED_A_CITY,
)
case(
    "response_schema_with_tools",
    "response_schema response_schema_with_tools tools",
    GENERATE,
    OPENAI,
    {**TOOLS, "response_schema": CITY},
    answer(completion(None, finish="tool_calls", tool_calls=[LOOKED_UP])),
    note="A round that calls tools is a step of the loop: its text is not checked.",
)
case(
    "response_schema_with_tools_behind_a_base_url",
    "response_schema response_schema_with_tools response_schema_unsupported before_request",
    GENERATE,
    LOCAL,
    {**TOOLS, "response_schema": CITY},
)
case(
    "response_schema_with_tools_said_to_be_taken",
    "response_schema response_schema_with_tools tools",
    GENERATE,
    {**LOCAL, "supports_response_schema_with_tools": True},
    {**TOOLS, "response_schema": CITY},
    answer(completion(PARIS, model="local-model")),
)
case(
    "response_schema_answer_that_does_not_fit",
    "response_schema response_schema_mismatch",
    GENERATE,
    OPENAI,
    ASKED_A_CITY,
    answer(completion("Paris is a city.")),
    note=PYTHON_WORDS,
)
case(
    "response_schema_answer_of_another_shape",
    "response_schema response_schema_mismatch",
    GENERATE,
    OPENAI,
    ASKED_A_CITY,
    answer(completion('{"town": "Paris"}')),
)
case(
    "response_schema_refusal",
    "response_schema refusal",
    GENERATE,
    OPENAI,
    ASKED_A_CITY,
    answer(completion(None, refusal="I cannot help with that.")),
)
case(
    "response_schema_truncated",
    "response_schema truncated finish_length",
    GENERATE,
    OPENAI,
    ASKED_A_CITY,
    answer(completion('{"city": "Par', finish="length")),
)
case(
    "response_schema_content_filter",
    "response_schema refusal finish_content_filter",
    GENERATE,
    OPENAI,
    ASKED_A_CITY,
    answer(completion("", finish="content_filter")),
)
case(
    "refusal_without_a_schema",
    "refusal",
    GENERATE,
    OPENAI,
    HELLO,
    answer(completion(None, refusal="I cannot help with that.")),
)

# -- The configuration on the wire ------------------------------------------------

case(
    "extra_body",
    "extra_body",
    GENERATE,
    {**LOCAL, "extra_body": {"top_k": 40, "temperature": 0.9, "model": "another-model"}},
    {**HELLO, "temperature": 0.2},
    answer(completion(model="local-model")),
    note="A member of extra_body replaces the provider's own of the same name.",
)
case(
    "default_headers",
    "default_headers",
    GENERATE,
    {**LOCAL, "default_headers": {"X-Route": "blue", "Accept": "application/x-ndjson"}},
    HELLO,
    answer(completion(model="local-model")),
)
case(
    "authorization_among_the_default_headers",
    "default_headers",
    GENERATE,
    {**LOCAL, "default_headers": {"Authorization": "Basic placeholder-basic"}},
    HELLO,
    answer(completion(model="local-model")),
)
case(
    "empty_key",
    "before_request",
    GENERATE,
    {**LOCAL, "api_key": ""},
    HELLO,
    sdk="The openai SDK builds no client without a key: the provider is never built.",
)
case(
    "key_of_the_request",
    "request_key",
    GENERATE,
    OPENAI,
    {**HELLO, "metadata": {"api_key": REQUEST_KEY, "tenant": "acme"}},
    answer(completion()),
    note="Python's OpenAI provider sends the configuration's key: it does not read the request's.",
)
case(
    "timeouts_of_the_configuration",
    "timeouts",
    GENERATE,
    {**LOCAL, "timeout": 120, "connect_timeout": 2.5},
    HELLO,
    answer(completion(model="local-model")),
)
case(
    "redirect",
    "redirect",
    GENERATE,
    LOCAL,
    HELLO,
    Reply(
        status=307, headers={"location": "https://llm.example/moved/v1/chat/completions"}, text=""
    ),
    answer(completion(model="local-model")),
    sdk="The openai SDK's client follows a redirect, each hop through the transport.",
)
case(
    "base_url_with_a_trailing_slash_and_a_query",
    "plain",
    GENERATE,
    {**LOCAL, "base_url": "https://llm.example/gateway/v1/?route=blue"},
    HELLO,
    answer(completion(model="local-model")),
)

# -- Usage, endings and empty answers --------------------------------------------------

CACHED = {
    "prompt_tokens": 1200,
    "completion_tokens": 45,
    "total_tokens": 1245,
    "prompt_tokens_details": {"cached_tokens": 800, "audio_tokens": 0},
    "completion_tokens_details": {"reasoning_tokens": 30, "audio_tokens": 0},
}
case(
    "usage_with_cached_and_reasoning_tokens",
    "usage_cached usage_reasoning",
    GENERATE,
    OPENAI,
    HELLO,
    answer(completion(usage=CACHED)),
)
case(
    "usage_with_a_cache_write",
    "usage_cached",
    GENERATE,
    LOCAL,
    HELLO,
    answer(
        completion(
            model="local-model",
            usage={
                "prompt_tokens": 1200,
                "completion_tokens": 45,
                "prompt_tokens_details": {"cached_tokens": 800, "cache_write_tokens": 300},
            },
        )
    ),
)
case(
    "usage_with_a_count_left_out",
    "usage_cached",
    GENERATE,
    LOCAL,
    HELLO,
    answer(
        completion(
            model="local-model",
            usage={"completion_tokens": 45, "prompt_tokens_details": {"cached_tokens": 800}},
        )
    ),
    note="A count the server leaves out stays unknown.",
)
case("no_usage", "plain", GENERATE, OPENAI, HELLO, answer(completion(usage=None)))
case(
    "ending_length",
    "finish_length truncated",
    GENERATE,
    OPENAI,
    HELLO,
    answer(completion("Hello th", finish="length")),
)
case(
    "ending_content_filter",
    "finish_content_filter",
    GENERATE,
    OPENAI,
    HELLO,
    answer(completion("", finish="content_filter")),
)
case("empty_answer", "empty_answer", GENERATE, OPENAI, HELLO, answer(completion("")))
case("null_answer", "empty_answer", GENERATE, OPENAI, HELLO, answer(completion(None, finish=None)))
case(
    "no_choice",
    "empty_answer",
    GENERATE,
    OPENAI,
    HELLO,
    answer({**completion(), "choices": []}),
    note="A response with no choice still reports its usage.",
)

# -- Failures -----------------------------------------------------------------------

BAD_REQUEST = failed(
    "Unsupported value: 'temperature'.",
    "invalid_request_error",
    "unsupported_value",
    "temperature",
)
for status, body in (
    (400, BAD_REQUEST),
    (401, failed("Incorrect API key provided.", "invalid_request_error", "invalid_api_key")),
    (
        403,
        failed(
            "Country not supported.", "request_forbidden", "unsupported_country_region_territory"
        ),
    ),
    (
        404,
        failed("The model `gpt-4o` does not exist.", "invalid_request_error", "model_not_found"),
    ),
    (408, failed("Request timed out.", "timeout")),
    (409, failed("The resource is locked.", "conflict")),
    (422, failed("Unprocessable entity.", "invalid_request_error")),
    (429, failed("Rate limit reached for requests.", "requests", "rate_limit_exceeded")),
    (500, failed("The server had an error.", "server_error")),
    (503, failed("The engine is currently overloaded.", "service_unavailable")),
):
    case(f"status_{status}", f"status_{status}", GENERATE, OPENAI, HELLO, answer(body, status))

LIMITED = failed("Rate limit reached for requests.", "requests", "rate_limit_exceeded")
case(
    "status_429_with_a_wait_in_seconds",
    "status_429 retry_after_seconds",
    GENERATE,
    OPENAI,
    HELLO,
    answer(LIMITED, 429, **{"retry-after": "7"}),
    note="Without retries of its own the provider does not read the wait: the error carries none.",
)
case(
    "status_429_with_a_wait_as_a_date",
    "status_429 retry_after_date",
    GENERATE,
    OPENAI,
    HELLO,
    answer(LIMITED, 429, **{"retry-after": "Thu, 01 Jan 2026 00:00:07 GMT"}),
)
case(
    "status_429_for_a_quota",
    "status_429",
    GENERATE,
    OPENAI,
    HELLO,
    answer(
        failed("You exceeded your current quota.", "insufficient_quota", "insufficient_quota"), 429
    ),
)
OVERFLOW = (
    "This model's maximum context length is 128000 tokens. "
    "However, your messages resulted in 130000 tokens."
)
case(
    "context_overflow_by_its_code",
    "context_overflow status_400",
    GENERATE,
    OPENAI,
    HELLO,
    answer(
        failed(
            "The request is too large for the model.",
            "invalid_request_error",
            "context_length_exceeded",
        ),
        400,
    ),
)
case(
    "context_overflow_by_its_words",
    "context_overflow status_400",
    GENERATE,
    LOCAL,
    HELLO,
    answer({"detail": OVERFLOW}, 400),
    note="No code says it: the error leaves context_overflow unsaid, and its message is read.",
)
case(
    "status_413_for_a_request_too_large",
    "status_413",
    GENERATE,
    OPENAI,
    HELLO,
    answer(failed("Request too large.", "invalid_request_error", "request_too_large"), 413),
)
case(
    "status_502_with_a_page",
    "not_json status_502",
    GENERATE,
    LOCAL,
    HELLO,
    page("<html><body>Bad gateway</body></html>", 502),
)
case(
    "status_500_with_an_empty_body",
    "not_json status_500",
    GENERATE,
    LOCAL,
    HELLO,
    page("", 500, "text/plain"),
)
case(
    "status_400_with_a_plain_text",
    "not_json status_400",
    GENERATE,
    LOCAL,
    HELLO,
    page("model not loaded", 400, "text/plain"),
)
case(
    "answer_200_that_is_a_page",
    "not_json",
    GENERATE,
    LOCAL,
    HELLO,
    page("<html><body>Sign in</body></html>", 200),
)
case(
    "answer_200_that_is_no_json",
    "not_json",
    GENERATE,
    LOCAL,
    HELLO,
    page("{not json", 200, "application/json"),
    note=PYTHON_WORDS,
)
case(
    "answer_200_that_is_no_completion",
    "not_json",
    GENERATE,
    LOCAL,
    HELLO,
    answer({"status": "ok"}),
)
case(
    "answer_200_that_is_an_error_object",
    "error_object",
    GENERATE,
    LOCAL,
    HELLO,
    answer(failed("The engine is currently overloaded.", "overloaded_error")),
    note="A gateway's 200 carrying an error reads as the status the error names.",
)
case(
    "answer_200_that_is_an_error_naming_no_status",
    "error_object",
    GENERATE,
    LOCAL,
    HELLO,
    answer({"error": "the model is loading"}),
)
case("connection_refused", "connection_refused", GENERATE, OPENAI, HELLO, REFUSED, sdk=SDK_WORDS)
case("timeout", "timeout", GENERATE, OPENAI, HELLO, TIMED_OUT, sdk=SDK_WORDS)

# -- Retries of the SDK ------------------------------------------------------------

RETRIED = {**OPENAI, "max_retries": 2}
OVERLOADED = failed("The engine is currently overloaded.", "service_unavailable")
case(
    "retry_that_succeeds",
    "retry_succeeds status_503",
    GENERATE,
    RETRIED,
    HELLO,
    answer(OVERLOADED, 503),
    answer(completion()),
    sdk=SDK_RETRIES,
)
case(
    "retries_exhausted",
    "retries_exhausted status_500",
    GENERATE,
    RETRIED,
    HELLO,
    answer(failed("The server had an error.", "server_error"), 500),
    answer(OVERLOADED, 503),
    answer(LIMITED, 429),
    sdk=SDK_RETRIES,
)
case(
    "retry_after_the_wait_asked_in_seconds",
    "retry_succeeds retry_after_seconds status_429",
    GENERATE,
    RETRIED,
    HELLO,
    answer(LIMITED, 429, **{"retry-after": "7"}),
    answer(completion()),
    sdk=SDK_RETRIES,
)
case(
    "retry_after_the_wait_asked_in_milliseconds",
    "retry_succeeds retry_after_seconds status_429",
    GENERATE,
    RETRIED,
    HELLO,
    answer(LIMITED, 429, **{"retry-after-ms": "1500", "retry-after": "7"}),
    answer(completion()),
    sdk=SDK_RETRIES,
)
case(
    "retry_after_the_wait_asked_as_a_date",
    "retry_succeeds retry_after_date status_429",
    GENERATE,
    RETRIED,
    HELLO,
    answer(LIMITED, 429, **{"retry-after": "Thu, 01 Jan 2026 00:00:07 GMT"}),
    answer(completion()),
    sdk=SDK_RETRIES,
)
case(
    "no_retry_past_two_minutes_of_wait",
    "status_429 retry_after_seconds",
    GENERATE,
    RETRIED,
    HELLO,
    answer(LIMITED, 429, **{"retry-after": "121"}),
    sdk=SDK_RETRIES,
)
case(
    "no_retry_of_a_final_status",
    "status_400",
    GENERATE,
    RETRIED,
    HELLO,
    answer(BAD_REQUEST, 400),
    sdk=SDK_RETRIES,
)
case(
    "no_retry_where_the_answer_says_so",
    "status_503",
    GENERATE,
    RETRIED,
    HELLO,
    answer(OVERLOADED, 503, **{"x-should-retry": "false"}),
    sdk=SDK_RETRIES,
)
case(
    "retry_where_the_answer_says_so",
    "retry_succeeds status_400",
    GENERATE,
    RETRIED,
    HELLO,
    answer(BAD_REQUEST, 400, **{"x-should-retry": "true"}),
    answer(completion()),
    sdk=SDK_RETRIES,
)
case(
    "retry_of_a_conflict_and_a_timeout_status",
    "retry_succeeds status_408 status_409",
    GENERATE,
    RETRIED,
    HELLO,
    answer(failed("Request timed out.", "timeout"), 408),
    answer(failed("The resource is locked.", "conflict"), 409),
    answer(completion()),
    sdk=SDK_RETRIES,
)
case(
    "retry_of_a_connection_refused",
    "retry_succeeds connection_refused",
    GENERATE,
    RETRIED,
    HELLO,
    REFUSED,
    answer(completion()),
    sdk=SDK_RETRIES,
)
case(
    "retry_of_a_timeout",
    "retry_succeeds timeout",
    GENERATE,
    RETRIED,
    HELLO,
    TIMED_OUT,
    answer(completion()),
    sdk=SDK_RETRIES,
)
case(
    "retries_exhausted_on_connections_refused",
    "retries_exhausted connection_refused",
    GENERATE,
    {**OPENAI, "max_retries": 1},
    HELLO,
    REFUSED,
    REFUSED,
    sdk=SDK_RETRIES,
)

# -- Streams ----------------------------------------------------------------------------

case(
    "stream",
    "stream stream_usage_on finish_stop",
    STREAM,
    OPENAI,
    HELLO,
    stream(said("Hel", "lo ", "there.")),
)
case("stream_as_text", "stream", TEXT, OPENAI, HELLO, stream(said("Hel", "lo ", "there.")))
case(
    "stream_without_usage",
    "stream stream_usage_off",
    STREAM,
    {**LOCAL, "include_stream_usage": False},
    HELLO,
    stream(said("Hello.", usage=None)),
)
case(
    "stream_request_of_a_whole_configuration",
    "stream tools reasoning_effort extra_body max_completion_tokens no_temperature",
    STREAM,
    {**MODERN, "reasoning_effort": "high", "extra_body": {"top_k": 40}},
    {
        **TOOLS,
        "system_prompt": "You answer in one sentence.",
        "temperature": 0.2,
        "max_tokens": 256,
    },
    stream(said("Hello.")),
)
case(
    "stream_read_a_few_bytes_at_a_time",
    "stream stream_cut_anywhere",
    STREAM,
    OPENAI,
    HELLO,
    stream(cut_every(said("Hel", "lo ", "th\u00e9re \u20ac."), 7)),
    note="The same events, whatever the reads cut: a line, an event, the end mark.",
)
case(
    "stream_in_one_read_with_other_line_ends",
    "stream stream_cut_anywhere",
    STREAM,
    OPENAI,
    HELLO,
    stream(
        [
            "".join(said("Hello.")).replace("\n", "\r\n")
            + ": keep-alive\r\n\r\nevent: ping\r\ndata: {}\r\n\r\n"
        ]
    ),
)
case(
    "stream_usage_with_cached_and_reasoning_tokens",
    "stream usage_cached usage_reasoning",
    STREAM,
    OPENAI,
    HELLO,
    stream(said("Hello.", usage=CACHED)),
)
case(
    "stream_cut_by_the_token_bound",
    "stream finish_length truncated",
    STREAM,
    OPENAI,
    HELLO,
    stream(said("Hello th", finish="length")),
)
case(
    "stream_content_filter",
    "stream finish_content_filter",
    STREAM,
    OPENAI,
    HELLO,
    stream(said(finish="content_filter")),
)
case("stream_empty_answer", "stream empty_answer", STREAM, OPENAI, HELLO, stream(said()))
case(
    "stream_that_ends_without_an_ending",
    "stream empty_answer",
    STREAM,
    LOCAL,
    HELLO,
    stream(
        sse(chunk({"role": "assistant", "content": "Hello."}, model="local-model"), done=False)
    ),
    note="No finish reason and no end mark: the stream's end is the answer's.",
)
case(
    "stream_think_tags_across_chunks",
    "stream thinking_read",
    STREAM,
    LOCAL,
    HELLO,
    stream(said("\n<th", "ink>They ", "greet me.</thi", "nk>Hello", ". <think>is text</think>")),
)
case(
    "stream_think_block_never_closed",
    "stream thinking_read finish_length",
    STREAM,
    LOCAL,
    HELLO,
    stream(said("<think>They gre", "et", finish="length")),
)
case(
    "stream_reasoning_in_its_own_field",
    "stream thinking_read",
    STREAM,
    LOCAL,
    HELLO,
    stream(
        sse(
            chunk({"role": "assistant", "content": "", "reasoning_content": "They "}),
            chunk({"reasoning_content": "greet me."}),
            chunk({"reasoning": " Then.", "content": "Hello."}),
            chunk({}, finish="stop"),
        )
    ),
)
CALL_STREAMED = sse(
    chunk({"role": "assistant", "content": None}),
    fragment(0, "call_1", "lookup_order", ""),
    fragment(0, None, None, '{"num'),
    fragment(0, None, None, 'ber": "A-1"}'),
    chunk({}, finish="tool_calls"),
    chunk(usage=USAGE),
)
case(
    "stream_tool_call_in_fragments",
    "stream tools tool_call tool_call_streamed finish_tool_calls",
    STREAM,
    OPENAI,
    TOOLS,
    stream(CALL_STREAMED),
)
case(
    "stream_tool_call_as_text",
    "stream tool_call_streamed",
    TEXT,
    OPENAI,
    TOOLS,
    stream(CALL_STREAMED),
)
case(
    "stream_tool_calls_beside_text",
    "stream tool_call tool_call_streamed tool_calls_several",
    STREAM,
    OPENAI,
    TOOLS,
    stream(
        sse(
            chunk({"role": "assistant", "content": "Let me "}),
            chunk({"content": "look."}),
            fragment(0, "call_1", "lookup_order", '{"number": '),
            fragment(1, "call_2", "clock", ""),
            fragment(0, None, None, '"A-1"}'),
            fragment(1, None, None, "{}"),
            chunk({}, finish="tool_calls"),
        )
    ),
)
case(
    "stream_tool_calls_whole_under_one_index",
    "stream tool_call tool_call_streamed tool_calls_several minted_call_id",
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            fragment(0, "call_1", "lookup_order", '{"number": "A-1"}'),
            fragment(0, "call_2", "clock", "{}"),
            fragment(0, None, "clock", "{}"),
            chunk({}, finish="tool_calls"),
        )
    ),
    note="A server that sends each call whole may tag every one with index 0.",
)
case(
    "stream_tool_call_without_an_id",
    "stream tool_call tool_call_streamed minted_call_id",
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            fragment(0, None, "lookup_order", '{"number": '),
            fragment(0, "call_late", None, '"A-1"}'),
            chunk({}, finish="tool_calls"),
        )
    ),
    note="The id the first event named the call by is kept.",
)
case(
    "stream_tool_call_with_arguments_that_never_read",
    "stream tool_call tool_call_streamed tool_call_unreadable",
    STREAM,
    OPENAI,
    TOOLS,
    stream(
        sse(
            fragment(0, "call_1", "lookup_order", "number "),
            fragment(0, None, None, "A-1"),
            chunk({}, finish="tool_calls"),
        )
    ),
)
case(
    "stream_tool_call_cut_by_the_token_bound",
    "stream tool_call tool_call_streamed tool_call_unreadable finish_length",
    STREAM,
    OPENAI,
    TOOLS,
    stream(
        sse(
            fragment(0, "call_1", "lookup_order", '{"number": "A-1"}'),
            fragment(1, "call_2", "lookup_order", '{"number": "A-'),
            chunk({}, finish="length"),
        )
    ),
)
case(
    "stream_response_schema",
    "stream response_schema",
    STREAM,
    OPENAI,
    ASKED_A_CITY,
    stream(said('{"city": ', '"Paris"}')),
)
case(
    "stream_response_schema_answer_that_does_not_fit",
    "stream response_schema response_schema_mismatch",
    STREAM,
    OPENAI,
    ASKED_A_CITY,
    stream(said("Paris is ", "a city.")),
    note=f"The error comes in place of the done event, after the text. {PYTHON_WORDS}",
)
case(
    "stream_response_schema_truncated",
    "stream response_schema truncated finish_length",
    STREAM,
    OPENAI,
    ASKED_A_CITY,
    stream(said('{"city": "Par', finish="length")),
)
case(
    "stream_response_schema_refusal",
    "stream response_schema refusal",
    STREAM,
    OPENAI,
    ASKED_A_CITY,
    stream(
        sse(
            chunk({"role": "assistant", "content": None, "refusal": "I cannot "}),
            chunk({"refusal": "help with that."}),
            chunk({}, finish="stop"),
        )
    ),
)
case(
    "stream_refusal_without_a_schema",
    "stream refusal",
    STREAM,
    OPENAI,
    HELLO,
    stream(
        sse(
            chunk({"role": "assistant", "content": None, "refusal": "I cannot help with that."}),
            chunk({}, finish="stop"),
        )
    ),
)
case(
    "stream_response_schema_where_the_server_takes_none",
    "stream response_schema response_schema_unsupported before_request",
    STREAM,
    {**LOCAL, "supports_response_schema": False},
    ASKED_A_CITY,
)

# -- Streams that fail -------------------------------------------------------------

case(
    "stream_cut_before_its_end",
    "stream stream_cut",
    STREAM,
    OPENAI,
    HELLO,
    stream(
        sse(chunk({"role": "assistant", "content": "Hel"}), chunk({"content": "lo"}), done=False),
        cut=True,
    ),
    note="The text read before the cut was given; the error follows it.",
)
case(
    "stream_cut_inside_an_event",
    "stream stream_cut",
    TEXT,
    OPENAI,
    HELLO,
    stream(
        [*sse(chunk({"content": "Hel"}), done=False), 'data: {"id":"chatcmpl-1","choi'], cut=True
    ),
)
case(
    "stream_that_sends_an_error_event",
    "stream stream_error_event",
    STREAM,
    OPENAI,
    HELLO,
    stream(sse(chunk({"content": "Hel"}), failed("The server had an error.", "server_error"))),
)
case(
    "stream_that_sends_an_error_naming_no_status",
    "stream stream_error_event",
    STREAM,
    LOCAL,
    HELLO,
    stream(sse(chunk({"content": "Hel"}), {"error": {"message": "The model crashed."}})),
)
case(
    "stream_that_sends_an_overload_event",
    "stream stream_error_event",
    STREAM,
    LOCAL,
    HELLO,
    stream(sse(failed("Overloaded.", "overloaded_error"))),
)
case(
    "stream_with_no_event",
    "stream not_json",
    STREAM,
    LOCAL,
    HELLO,
    stream([]),
)
case(
    "stream_answered_with_a_page",
    "stream not_json",
    STREAM,
    LOCAL,
    HELLO,
    page("<html><body>Sign in</body></html>", 200),
)
case(
    "stream_answered_with_a_json_error_object",
    "stream error_object",
    STREAM,
    LOCAL,
    HELLO,
    answer(failed("The engine is currently overloaded.", "overloaded_error")),
)
case(
    "stream_event_that_is_no_json",
    "stream not_json",
    STREAM,
    LOCAL,
    HELLO,
    stream(
        [*sse(chunk({"content": "Hel"}), done=False), "data: {not json\n\n", "data: [DONE]\n\n"]
    ),
    note=PYTHON_WORDS,
)
case("stream_status_429", "stream status_429", STREAM, OPENAI, HELLO, answer(LIMITED, 429))
case(
    "stream_context_overflow",
    "stream context_overflow status_400",
    TEXT,
    OPENAI,
    HELLO,
    answer(failed(OVERFLOW, "invalid_request_error", "context_length_exceeded"), 400),
)
case(
    "stream_connection_refused",
    "stream connection_refused",
    STREAM,
    OPENAI,
    HELLO,
    REFUSED,
    sdk=SDK_WORDS,
)
case("stream_timeout", "stream timeout", STREAM, OPENAI, HELLO, TIMED_OUT, sdk=SDK_WORDS)
case(
    "stream_retry_that_succeeds",
    "stream retry_succeeds status_503",
    STREAM,
    RETRIED,
    HELLO,
    answer(OVERLOADED, 503),
    stream(said("Hello.")),
    sdk=SDK_RETRIES,
)
case(
    "stream_cut_is_not_tried_again",
    "stream stream_cut",
    STREAM,
    RETRIED,
    HELLO,
    stream(sse(chunk({"content": "Hel"}), done=False), cut=True),
    sdk="A stream is tried again until its head came, never after.",
)
