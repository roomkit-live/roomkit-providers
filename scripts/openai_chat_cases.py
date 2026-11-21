"""The cases ``generate_openai_chat_cases.py`` runs on RoomKit's Python OpenAI
provider: for each, a configuration, a request, and what the fake endpoint
answers, in order.

A case names its ``kinds``: what it shows of the provider. The Rust test that
reads the fixture lists the kinds it requires, so a table that loses one, or
names one the test does not know, fails there.
"""

from __future__ import annotations

import json
from typing import Any

from wire_capture import (
    GENERATE,
    JSON,
    KEY,
    LISTING,
    LOST,
    REFUSED,
    REQUEST_KEY,
    STREAM,
    TEXT,
    TIMED_OUT,
    TIMEOUT,
    Case,
    Reply,
    annotate,
    answer,
    cut_every,
    cut_inside_characters,
    half,
    one_byte_at_a_time,
    page,
    registrar,
    stream,
)

SDK_RETRIES = (
    "The openai SDK decides which answers it tries again, how many times and after what wait."
)
SDK_WORDS = "No answer came: the message is the openai SDK's."
PYTHON_WORDS = "The message holds what Python's JSON reader says of the text, in its words."

CASES: list[Case] = []
case = registrar(CASES)


def raw_call(**members: Any) -> dict[str, Any]:
    """A streamed entry of ``tool_calls``, written member by member."""
    return chunk({"tool_calls": [members]})


# -- What the fake answers ---------------------------------------------------

USAGE = {"prompt_tokens": 12, "completion_tokens": 5, "total_tokens": 17}
ANSWERED_BY = "gpt-4o-2024-08-06"


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
    model: str | None = ANSWERED_BY,
) -> dict[str, Any]:
    choices = [] if delta is None else [{"index": 0, "delta": delta, "finish_reason": finish}]
    body: dict[str, Any] = {
        "id": "chatcmpl-1",
        "object": "chat.completion.chunk",
        "created": 1700000000,
        "model": model,
        "choices": choices,
    }
    if model is None:
        del body["model"]
    if usage is not None:
        body["usage"] = usage
    return body


def sse(*events: Any, done: bool = True, escaped: bool = True) -> list[str]:
    """Each event as one read of the stream, then the end mark. A character
    past ASCII is written as it is unless ``escaped``."""
    written = [json.dumps(event, separators=(",", ":"), ensure_ascii=escaped) for event in events]
    lines = [f"data: {event}\n\n" for event in written]
    return [*lines, "data: [DONE]\n\n"] if done else lines


def said(
    *texts: str,
    finish: str = "stop",
    usage: dict[str, Any] | None = USAGE,
    escaped: bool = True,
) -> list[str]:
    """A streamed answer: the role, each text, the end, then the usage."""
    events = [chunk({"role": "assistant", "content": ""})]
    events += [chunk({"content": text}) for text in texts]
    events.append(chunk({}, finish=finish))
    if usage is not None:
        events.append(chunk(usage=usage))
    return sse(*events, escaped=escaped)


def fragment(index: int, call_id: str | None, name: str | None, arguments: str) -> dict[str, Any]:
    """A streamed fragment of a tool call."""
    function = {"arguments": arguments} if name is None else {"name": name, "arguments": arguments}
    call: dict[str, Any] = {"index": index, "function": function}
    if call_id is not None:
        call = {"index": index, "id": call_id, "type": "function", "function": function}
    return chunk({"tool_calls": [call]})


def failed(message: str, kind: str, code: str | None = None, param: str | None = None) -> Any:
    return {"error": {"message": message, "type": kind, "param": param, "code": code}}


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
    {**LOCAL, "extra_body": {"top_k": 40, "min_p": 0.05}},
    {**HELLO, "temperature": 0.2},
    answer(completion(model="local-model")),
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
case(
    "base_url_with_two_trailing_slashes",
    "plain",
    GENERATE,
    {**LOCAL, "base_url": "https://llm.example/gateway/v1//"},
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
case(
    "ending_empty_finish_reason",
    "finish_empty",
    GENERATE,
    LOCAL,
    HELLO,
    answer(completion(finish="", model="local-model")),
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
# What a server that validates a request field by field answers one it refuses.
UNVALIDATED = {
    "detail": [
        {
            "type": "missing",
            "loc": ["body", "messages"],
            "msg": "Field required",
            "input": {"model": "local-model"},
        }
    ]
}
case(
    "status_422_whose_detail_is_a_list",
    "detail_list status_422",
    GENERATE,
    LOCAL,
    HELLO,
    answer(UNVALIDATED, 422),
    note="The detail is a list, an entry for each field refused, where another server's is a text.",
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
    "not_a_completion",
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
    stream(sse(chunk({"role": "assistant", "content": "Hello."}, model="local-model"), done=False)),
    note="No finish reason and no end mark: the stream's end is the answer's.",
)
# A finish reason that is an empty text, as some servers send for the last chunk.
ENDED_FOR_NO_REASON = sse(
    chunk({"role": "assistant", "content": "Hello."}, model="local-model"),
    chunk({}, finish="", model="local-model"),
    done=False,
)
case(
    "stream_empty_finish_reason_without_an_end_mark",
    "stream finish_empty",
    STREAM,
    LOCAL,
    HELLO,
    stream(ENDED_FOR_NO_REASON),
)
case(
    "stream_empty_finish_reason_with_its_end_mark",
    "stream finish_empty",
    STREAM,
    LOCAL,
    HELLO,
    stream([*ENDED_FOR_NO_REASON, "data: [DONE]\n\n"]),
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
        cut=LOST,
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
        [*sse(chunk({"content": "Hel"}), done=False), 'data: {"id":"chatcmpl-1","choi'], cut=LOST
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
    "stream_status_422_whose_detail_is_a_list",
    "stream detail_list status_422",
    STREAM,
    LOCAL,
    HELLO,
    answer(UNVALIDATED, 422),
)
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
    stream(sse(chunk({"content": "Hel"}), done=False), cut=LOST),
    sdk="A stream is tried again until its head came, never after.",
)

# -- What extra_body may replace -------------------------------------------------

case(
    "extra_body_replacing_the_request_s_own",
    "extra_body",
    GENERATE,
    {
        **LOCAL,
        "extra_body": {
            "model": "another-model",
            "temperature": 0.9,
            "messages": [{"role": "user", "content": "Something else"}],
        },
    },
    {**HELLO, "temperature": 0.2},
    answer(completion(model="another-model")),
)

# -- Reads that cut inside a character --------------------------------------------

E_ACUTE, EURO, GRIN = chr(0xE9), chr(0x20AC), chr(0x1F600)
# Characters of two, three and four bytes.
WIDE = f"caf{E_ACUTE} {EURO} {GRIN}"
INSIDE = "stream stream_cut_inside_a_character"


case(
    "stream_text_cut_inside_characters",
    INSIDE,
    STREAM,
    OPENAI,
    HELLO,
    stream(cut_inside_characters(said("Un ", WIDE, ".", escaped=False))),
)
case(
    "stream_call_arguments_cut_inside_characters",
    f"{INSIDE} tool_call_streamed",
    STREAM,
    OPENAI,
    TOOLS,
    stream(
        cut_inside_characters(
            sse(
                fragment(0, "call_1", "lookup_order", '{"number": "'),
                fragment(0, None, None, f'{WIDE}"}}'),
                chunk({}, finish="tool_calls"),
                escaped=False,
            )
        )
    ),
)
case(
    "stream_reasoning_cut_inside_characters",
    f"{INSIDE} thinking_read",
    STREAM,
    LOCAL,
    HELLO,
    stream(
        cut_inside_characters(
            sse(
                chunk({"reasoning_content": WIDE}),
                chunk({"content": "Oui."}),
                chunk({}, finish="stop"),
                escaped=False,
            )
        )
    ),
)
case(
    "stream_one_byte_per_read",
    f"{INSIDE} stream_cut_anywhere",
    STREAM,
    OPENAI,
    HELLO,
    stream(one_byte_at_a_time(said(WIDE, usage=None, escaped=False))),
)

# -- A body that fails midway -------------------------------------------------------

AFTER_ITS_HEAD = "A stream is tried again until its head came, never after."
case(
    "answer_lost_midway",
    "body_lost",
    GENERATE,
    OPENAI,
    HELLO,
    half(completion(), LOST),
    sdk=SDK_WORDS,
)
case(
    "answer_lost_midway_on_every_try",
    "body_lost retries_exhausted",
    GENERATE,
    RETRIED,
    HELLO,
    half(completion(), LOST),
    half(completion(), LOST),
    half(completion(), LOST),
    sdk=SDK_RETRIES,
)
case(
    "answer_lost_midway_then_whole",
    "body_lost retry_succeeds",
    GENERATE,
    RETRIED,
    HELLO,
    half(completion(), LOST),
    answer(completion()),
    sdk=SDK_RETRIES,
)
case(
    "answer_timed_out_midway_on_every_try",
    "body_lost timeout retries_exhausted",
    GENERATE,
    {**OPENAI, "max_retries": 1},
    HELLO,
    half(completion(), TIMEOUT),
    half(completion(), TIMEOUT),
    sdk=SDK_RETRIES,
)
case(
    "stream_read_timeout_after_an_event",
    "stream stream_cut timeout",
    STREAM,
    RETRIED,
    HELLO,
    stream(sse(chunk({"content": "Hel"}), done=False), cut=TIMEOUT),
    sdk=AFTER_ITS_HEAD,
)
case(
    "stream_status_400_whose_body_is_lost",
    "stream status_400 body_lost",
    STREAM,
    OPENAI,
    HELLO,
    Reply(status=400, headers=dict(JSON), chunks=['{"error": {"mess'], cut=LOST),
    sdk="The openai SDK reads the body of a refused stream request before it raises.",
)

# -- How a streamed call is folded ---------------------------------------------------

FOLDING = "stream tool_call_streamed tool_call_folding"
ENDED = chunk({}, finish="tool_calls")
case(
    "stream_second_object_under_one_index",
    FOLDING,
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            fragment(0, "call_1", "clock", '{"zone": "utc"}'),
            fragment(0, None, None, '{"zone": "local"}'),
            ENDED,
        )
    ),
    note="A new JSON object after whole arguments, under no id and no name: another call.",
)
case(
    "stream_another_name_under_one_index",
    FOLDING,
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            fragment(0, "call_1", "lookup_order", '{"number": "A-1"}'),
            fragment(0, None, "clock", "{}"),
            ENDED,
        )
    ),
)
case(
    "stream_call_id_alone_then_its_name",
    FOLDING,
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            raw_call(index=0, id="call_1", type="function"),
            fragment(0, None, "clock", "{}"),
            ENDED,
        )
    ),
)
case(
    "stream_call_arguments_as_an_object",
    FOLDING,
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            raw_call(
                index=0,
                id="call_1",
                type="function",
                function={"name": "lookup_order", "arguments": {"number": "A-1"}},
            ),
            ENDED,
        )
    ),
)
case(
    "stream_call_indexes_far_apart",
    FOLDING,
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            fragment(7, "call_7", "clock", "{}"),
            fragment(3, "call_3", "lookup_order", '{"number": '),
            fragment(3, None, None, '"A-1"}'),
            ENDED,
        )
    ),
    note="The events number the calls in the order they came, not by their wire index.",
)
case(
    "stream_call_with_explicit_nulls",
    FOLDING,
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            raw_call(index=0, id=None, type="function", function={"name": None, "arguments": None}),
            raw_call(
                index=None, id="call_1", type=None, function={"name": "clock", "arguments": None}
            ),
            fragment(0, None, None, "{}"),
            ENDED,
        )
    ),
)
case(
    "stream_call_name_repeated_on_every_fragment",
    FOLDING,
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            fragment(0, "call_1", "lookup_order", '{"number": '),
            fragment(0, "call_1", "lookup_order", '"A-1"}'),
            fragment(0, None, "lookup_order", " "),
            ENDED,
        )
    ),
)
case(
    "stream_call_entry_with_no_function",
    FOLDING,
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            raw_call(index=0, id="call_1", type="custom", custom={"name": "x", "input": "y"}),
            chunk({"content": "Done."}),
            ENDED,
        )
    ),
)
case(
    "stream_tool_call_that_ends_without_an_ending",
    "stream tool_call_streamed",
    STREAM,
    LOCAL,
    TOOLS,
    stream(sse(fragment(0, "call_1", "lookup_order", '{"number": "A-1"}'), done=False)),
)
case(
    "stream_tool_call_empty_finish_reason_without_an_end_mark",
    "stream tool_call_streamed finish_empty",
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            fragment(0, "call_1", "lookup_order", '{"number": "A-1"}'),
            chunk({}, finish=""),
            done=False,
        )
    ),
)

# -- Think tags, held and not ----------------------------------------------------------

THOUGHTS = "thinking_read"
case(
    "stream_ends_holding_the_start_of_a_think_tag",
    f"stream {THOUGHTS}",
    STREAM,
    LOCAL,
    HELLO,
    stream(said("<thi")),
    note="A start that never became the tag is text.",
)
case(
    "stream_ends_holding_the_start_of_a_closing_tag",
    f"stream {THOUGHTS}",
    STREAM,
    LOCAL,
    HELLO,
    stream(said("<think>They greet me.", "</thi")),
    note="The end of a block never closed is reasoning.",
)
case(
    "stream_empty_think_block",
    f"stream {THOUGHTS}",
    STREAM,
    LOCAL,
    HELLO,
    stream(said("<think>", "</think>", "Hello.")),
)
for name, text in (
    ("think_tag_after_text", "Hello. <think>is text</think> Bye."),
    ("thinking_tag_of_another_name", "<thinking>A plan.</thinking>Hello."),
    ("empty_think_block", "<think></think>Hello."),
    ("think_block_after_blank_lines_and_tabs", " \t\r\n<think>They greet me.</think>Hello."),
):
    case(name, THOUGHTS, GENERATE, LOCAL, HELLO, answer(completion(text, model="local-model")))

# -- A call's arguments ----------------------------------------------------------------

ARGUMENTS = "tool_call call_arguments"
case(
    "call_arguments_with_numbers_json_does_not_name",
    ARGUMENTS,
    GENERATE,
    OPENAI,
    TOOLS,
    answer(
        completion(
            None,
            finish="tool_calls",
            tool_calls=[
                called("call_1", "lookup_order", '{"number": NaN}'),
                called("call_2", "lookup_order", '{"number": 1e999}'),
                called("call_3", "lookup_order", '{"number": -Infinity}'),
            ],
        )
    ),
)
case(
    "call_arguments_that_are_no_text",
    ARGUMENTS,
    GENERATE,
    LOCAL,
    TOOLS,
    answer(
        completion(
            None,
            finish="tool_calls",
            model="local-model",
            tool_calls=[
                called("call_1", "lookup_order", {"number": "A-1"}),
                called("call_2", "clock", None),
                called("call_3", "clock", "null"),
                called("call_4", "clock", '"text"'),
                called("call_5", "clock", 7),
            ],
        )
    ),
)
case(
    "round_replayed_with_arguments_of_several_members",
    "tool_round_replayed call_arguments",
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
                        "arguments": {
                            "number": "A-1",
                            "city": f"Montr{E_ACUTE}al",
                            "note": WIDE,
                            "quote": 'say "hi"\n',
                            "weight": 1.5,
                            "count": 3,
                            "large": 1e22,
                            "gift": True,
                            "none": None,
                            "tags": ["a", "b"],
                            "nested": {"b": 1, "a": [1.0, 2]},
                        },
                    }
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
    },
    answer(completion("It shipped.")),
    note=(
        "A call's arguments go back as Python's json.dumps writes them: its separators, "
        "its escapes past ASCII, its numbers."
    ),
)

# -- Usage in other shapes ---------------------------------------------------------------


def used(usage: Any) -> Reply:
    return answer(completion(model="local-model", usage=usage))


SHAPES = "usage_shapes"
for name, usage in (
    (
        "usage_cache_write_beside_the_details",
        {"prompt_tokens": 1200, "completion_tokens": 45, "cache_write_tokens": 300},
    ),
    (
        "usage_cache_write_that_is_no_whole_number",
        {"prompt_tokens": 1200, "completion_tokens": 45, "cache_write_tokens": 1.5},
    ),
    (
        "usage_cache_write_written_as_text_in_the_details",
        {
            "prompt_tokens": 1200,
            "completion_tokens": 45,
            "prompt_tokens_details": {"cache_write_tokens": "300"},
        },
    ),
    (
        "usage_cached_above_the_prompt",
        {
            "prompt_tokens": 100,
            "completion_tokens": 45,
            "prompt_tokens_details": {"cached_tokens": 800},
        },
    ),
    ("usage_without_completion_tokens", {"prompt_tokens": 12}),
    ("usage_counts_below_zero", {"prompt_tokens": -12, "completion_tokens": -5}),
):
    case(name, SHAPES, GENERATE, LOCAL, HELLO, used(usage))
case(
    "stream_usage_on_chunks_that_hold_a_choice",
    f"stream {SHAPES}",
    STREAM,
    LOCAL,
    HELLO,
    stream(
        sse(
            chunk({"content": "Hel"}, usage={"prompt_tokens": 1, "completion_tokens": 1}),
            chunk(
                {"content": "lo."},
                finish="stop",
                usage={"prompt_tokens": 12, "completion_tokens": 5},
            ),
        )
    ),
    note="The last usage a chunk carried is the stream's.",
)
case(
    "stream_chunks_naming_no_model",
    f"stream {SHAPES}",
    STREAM,
    LOCAL,
    HELLO,
    stream(sse(chunk({"content": "Hello."}, model=None), chunk({}, finish="stop", model=None))),
)

# -- The reasoning switch and its order -------------------------------------------------

SWITCH = "reasoning_effort reasoning_switch"
GPT_5 = {"api_key": KEY, "model": "gpt-5", "reasoning_effort": "high"}
HIGH = {**MODERN, "reasoning_effort": "high"}
for name, config, request in (
    ("thinking_budget_of_zero", GPT_5, {"thinking_budget": 0}),
    ("thinking_budget_above_zero", HIGH, {"thinking_budget": 2048}),
    (
        "thinking_budget_over_the_switch_and_the_effort",
        GPT_5,
        {"thinking_budget": 2048, "enable_thinking": False, "reasoning_effort": "medium"},
    ),
    (
        "thinking_budget_of_zero_over_the_switch",
        GPT_5,
        {"thinking_budget": 0, "enable_thinking": True, "reasoning_effort": "medium"},
    ),
    (
        "switch_on_over_an_effort_of_none",
        GPT_5,
        {"enable_thinking": True, "reasoning_effort": "none"},
    ),
    ("switch_off_over_an_effort", GPT_5, {"enable_thinking": False, "reasoning_effort": "medium"}),
    ("reasoning_turned_off_on_a_model_that_can", HIGH, {"enable_thinking": False}),
    ("effort_of_none_on_a_model_that_can", HIGH, {"reasoning_effort": "none"}),
    ("effort_behind_a_base_url_without_tools", {**LOCAL, "reasoning_effort": "high"}, {}),
    (
        "switch_off_behind_a_base_url",
        {**LOCAL, "reasoning_effort": "high"},
        {"enable_thinking": False},
    ),
    (
        "effort_on_a_model_that_takes_no_tools_without_tools",
        {"api_key": KEY, "model": "gpt-6-astra"},
        {"reasoning_effort": "none"},
    ),
):
    case(name, SWITCH, GENERATE, config, {**HELLO, **request}, answer(completion()))
case(
    "tools_and_effort_on_a_model_the_catalogue_does_not_tag",
    f"{SWITCH} tools",
    GENERATE,
    {**OPENAI, "reasoning_effort": "high"},
    TOOLS,
    answer(completion()),
    note="Left out, and warned of once in Python's log.",
)

# -- Refusals before the request, in their order -----------------------------------------

BAD_TOOL = {"name": "orders.lookup", "description": "Find an order"}
BAD_IMAGE = {"role": "user", "content": [{"type": "image", "url": "data:image/png;base64,***"}]}
EVERY_FAULT = {"messages": [BAD_IMAGE], "tools": [BAD_TOOL], "response_schema": CITY}
NO_SCHEMA = {"supports_response_schema": False}
case(
    "refusals_the_model_first",
    "before_request",
    GENERATE,
    {"api_key": KEY, "model": "gpt-5.5-pro", **NO_SCHEMA},
    EVERY_FAULT,
    note="Of a model, an image, a schema and a tool name to refuse, the model is refused.",
)
case(
    "refusals_the_image_next",
    "before_request image_data_uri",
    GENERATE,
    {**OPENAI, **NO_SCHEMA},
    EVERY_FAULT,
)
case(
    "refusals_the_schema_before_the_tool_name",
    "before_request response_schema_unsupported",
    GENERATE,
    {**OPENAI, **NO_SCHEMA},
    {**HELLO, "tools": [BAD_TOOL], "response_schema": CITY},
)
case(
    "refusals_in_a_stream_the_schema_before_the_tool_name",
    "stream before_request response_schema_unsupported",
    STREAM,
    {**OPENAI, **NO_SCHEMA},
    {**HELLO, "tools": [BAD_TOOL], "response_schema": CITY},
)

# -- A response schema, on the paths left ---------------------------------------------

case(
    "stream_response_schema_with_a_tool_call",
    "stream response_schema response_schema_with_tools tool_call_streamed",
    STREAM,
    OPENAI,
    {**TOOLS, "response_schema": CITY},
    stream(CALL_STREAMED),
    note="A round that calls tools is a step of the loop: its text is not checked.",
)
case(
    "stream_response_schema_content_filter",
    "stream response_schema refusal finish_content_filter",
    STREAM,
    OPENAI,
    ASKED_A_CITY,
    stream(said(finish="content_filter")),
)
case(
    "response_schema_with_no_choice",
    "response_schema empty_answer",
    GENERATE,
    OPENAI,
    ASKED_A_CITY,
    answer({**completion(), "choices": []}),
)

# -- Errors read from a body ---------------------------------------------------------------

case(
    "answer_200_that_is_an_error_with_a_status_for_code",
    "error_object",
    GENERATE,
    LOCAL,
    HELLO,
    answer({"error": {"message": "Try again later.", "code": 503}}),
)
case(
    "answer_200_that_is_an_error_with_a_status_as_text",
    "error_object",
    GENERATE,
    LOCAL,
    HELLO,
    answer({"error": {"message": "Slow down.", "code": "429"}}),
)
for name, message in (
    ("a_rate_limit", "Rate limit reached, slow down."),
    ("too_many_requests", "Too many requests."),
    ("an_overload", "The engine is overloaded."),
    ("a_status_worth_a_retry", "Upstream answered 502."),
    ("a_final_status", "Upstream answered 404."),
    ("a_token_limit", "The token limit was reached."),
    ("a_longer_number", "Request 5021 failed to generate."),
    ("a_longer_word", "The engine is overloadedness itself."),
):
    case(
        f"stream_error_that_names_{name}",
        "stream stream_error_event transient_words",
        STREAM,
        LOCAL,
        HELLO,
        stream(sse({"error": {"message": message}})),
        sdk="The openai SDK raises a stream's error event with the event's message.",
    )
case(
    "status_529",
    "status_529",
    GENERATE,
    OPENAI,
    HELLO,
    answer(failed("Overloaded.", "overloaded_error"), 529),
)
case("status_204", "status_204", GENERATE, LOCAL, HELLO, Reply(status=204, text=""))

# -- Answers in other shapes ---------------------------------------------------------------

ODD = "odd_shapes"
case(
    "message_null",
    f"{ODD} empty_answer",
    GENERATE,
    LOCAL,
    HELLO,
    answer({**completion(), "choices": [{"index": 0, "message": None, "finish_reason": "stop"}]}),
)
case(
    "choices_that_are_no_list",
    ODD,
    GENERATE,
    LOCAL,
    HELLO,
    answer({**completion(), "choices": "none"}),
)
case(
    "nulls_for_absent_members",
    ODD,
    GENERATE,
    LOCAL,
    HELLO,
    answer(
        {
            "id": None,
            "object": "chat.completion",
            "created": None,
            "model": None,
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello.",
                        "tool_calls": None,
                        "refusal": None,
                        "reasoning_content": None,
                    },
                    "finish_reason": None,
                    "logprobs": None,
                }
            ],
            "usage": None,
            "system_fingerprint": None,
        }
    ),
)
case(
    "stream_nulls_for_absent_members",
    f"stream {ODD}",
    STREAM,
    LOCAL,
    HELLO,
    stream(
        sse(
            {
                "id": None,
                "object": "chat.completion.chunk",
                "model": None,
                "choices": [
                    {
                        "index": 0,
                        "delta": {
                            "role": None,
                            "content": "Hello.",
                            "tool_calls": None,
                            "refusal": None,
                            "reasoning_content": None,
                        },
                        "finish_reason": None,
                    }
                ],
                "usage": None,
            },
            {"choices": None},
            chunk({}, finish="stop"),
        )
    ),
)

# -- The request, in other shapes -----------------------------------------------------------

case(
    "tool_parameters_without_a_type",
    "tools",
    GENERATE,
    OPENAI,
    {
        **HELLO,
        "tools": [
            {
                "name": "lookup_order",
                "description": "Find an order",
                "parameters": {
                    "properties": {"number": {"type": "string"}},
                    "required": ["number"],
                },
            },
            {"name": "clock", "description": "Tell the time", "parameters": {}},
        ],
    },
    answer(completion()),
    note="A root without a type is declared an object's.",
)
case(
    "token_bound_of_zero_in_the_request",
    "plain",
    GENERATE,
    OPENAI,
    {**HELLO, "max_tokens": 0},
    answer(completion()),
)
case(
    "token_bound_of_zero_in_the_configuration",
    "plain",
    GENERATE,
    {**OPENAI, "max_tokens": 0},
    HELLO,
    answer(completion()),
)

# -- Streams in other shapes -----------------------------------------------------------------

LAST = json.dumps(chunk({"content": "lo."}, finish="stop"), separators=(",", ":"))
case(
    "stream_last_event_without_its_blank_line",
    "stream stream_shapes",
    STREAM,
    LOCAL,
    HELLO,
    stream([*sse(chunk({"content": "Hel"}), done=False), f"data: {LAST}\n"]),
)
case(
    "stream_last_event_without_its_line_end",
    "stream stream_shapes",
    STREAM,
    LOCAL,
    HELLO,
    stream([*sse(chunk({"content": "Hel"}), done=False), f"data: {LAST}"]),
)
case(
    "stream_that_starts_with_a_byte_order_mark",
    "stream stream_shapes",
    STREAM,
    LOCAL,
    HELLO,
    stream([chr(0xFEFF) + "".join(sse(chunk({"content": "Hello."}), chunk({}, finish="stop")))]),
)
case(
    "stream_of_the_end_mark_alone",
    "stream stream_shapes",
    STREAM,
    LOCAL,
    HELLO,
    stream(["data: [DONE]\n\n"]),
)

# -- A redirect to another host --------------------------------------------------------------

case(
    "redirect_302_to_another_host",
    "redirect",
    GENERATE,
    LOCAL,
    HELLO,
    Reply(
        status=302, headers={"location": "https://elsewhere.example/v1/chat/completions"}, text=""
    ),
    answer(completion(model="local-model")),
    sdk="The openai SDK's client follows a redirect, each hop through the transport.",
)

# -- The waits between two tries ----------------------------------------------------------------

WAIT = "retry_wait"
case(
    "retry_waits_double_up_to_eight_seconds",
    f"{WAIT} retries_exhausted status_503",
    GENERATE,
    {**OPENAI, "max_retries": 6},
    HELLO,
    *[answer(OVERLOADED, 503)] * 7,
    sdk=SDK_RETRIES,
)
for name, headers, tried_again in (
    ("of_exactly_two_minutes", {"retry-after": "120"}, True),
    ("of_zero", {"retry-after": "0"}, True),
    ("below_zero", {"retry-after": "-5"}, True),
    ("that_does_not_read", {"retry-after": "soon"}, True),
    ("as_a_date_already_past", {"retry-after": "Wed, 31 Dec 2025 23:59:00 GMT"}, True),
    ("in_milliseconds_that_do_not_read", {"retry-after-ms": "soon", "retry-after": "3"}, True),
    (
        "past_two_minutes_where_the_answer_says_to_retry",
        {"retry-after": "121", "x-should-retry": "true"},
        False,
    ),
):
    again = [answer(completion())] if tried_again else []
    case(
        f"retry_wait_{name}",
        f"{WAIT} status_429 retry_after_seconds",
        GENERATE,
        RETRIED,
        HELLO,
        answer(LIMITED, 429, **headers),
        *again,
        sdk=SDK_RETRIES,
    )
for status in (502, 504):
    case(
        f"retry_of_status_{status}",
        f"{WAIT} retry_succeeds status_{status}",
        GENERATE,
        RETRIED,
        HELLO,
        answer(failed("Upstream failed.", "server_error"), status),
        answer(completion()),
        sdk=SDK_RETRIES,
    )

# -- Counts that are no whole number ------------------------------------------------------

PRICED = {"prompt_tokens": 1200, "completion_tokens": 45}
MALFORMED_USAGE: tuple[tuple[str, Any], ...] = (
    ("prompt_tokens_as_text", {"prompt_tokens": "12", "completion_tokens": 5}),
    ("completion_tokens_as_text", {"prompt_tokens": 12, "completion_tokens": "5"}),
    ("completion_tokens_as_a_word", {"prompt_tokens": 12, "completion_tokens": "five"}),
    ("counts_with_a_fraction", {"prompt_tokens": 12.5, "completion_tokens": 5.5}),
    ("counts_as_whole_floats", {"prompt_tokens": 12.0, "completion_tokens": 5.0}),
    ("counts_that_are_true", {"prompt_tokens": True, "completion_tokens": True}),
    ("cached_tokens_as_text", {**PRICED, "prompt_tokens_details": {"cached_tokens": "800"}}),
    ("cached_tokens_as_a_word", {**PRICED, "prompt_tokens_details": {"cached_tokens": "many"}}),
    (
        "reasoning_tokens_as_text",
        {**PRICED, "completion_tokens_details": {"reasoning_tokens": "30"}},
    ),
    (
        "reasoning_tokens_as_a_word",
        {**PRICED, "completion_tokens_details": {"reasoning_tokens": "many"}},
    ),
    ("cache_write_beside_the_details_as_text", {**PRICED, "cache_write_tokens": "300"}),
    ("that_is_a_text", "12 tokens"),
    (
        "details_that_are_a_text",
        {**PRICED, "prompt_tokens_details": "none", "completion_tokens_details": "none"},
    ),
)
for name, usage in MALFORMED_USAGE:
    case(f"usage_{name}", SHAPES, GENERATE, LOCAL, HELLO, used(usage))
    case(
        f"stream_usage_{name}",
        f"stream {SHAPES}",
        STREAM,
        LOCAL,
        HELLO,
        stream(said("Hello.", usage=usage)),
    )

# -- A body lost before it starts, and a refusal whose body is lost ------------------------

case(
    "answer_lost_before_any_byte",
    "body_lost",
    GENERATE,
    OPENAI,
    HELLO,
    Reply(status=200, headers=dict(JSON), chunks=[], cut=LOST),
    sdk=SDK_WORDS,
)
case(
    "stream_lost_before_any_byte",
    "stream body_lost stream_cut",
    STREAM,
    OPENAI,
    HELLO,
    stream([], cut=LOST),
    sdk=SDK_WORDS,
)
LOST_400 = Reply(status=400, headers=dict(JSON), chunks=['{"error": {"mess'], cut=LOST)
READ_BEFORE_IT_RAISES = "The openai SDK reads the body of a refused request before it raises."
case(
    "status_400_whose_body_is_lost",
    "status_400 body_lost",
    GENERATE,
    OPENAI,
    HELLO,
    LOST_400,
    sdk=READ_BEFORE_IT_RAISES,
)
case(
    "status_400_whose_body_is_lost_on_every_try",
    "status_400 body_lost retries_exhausted",
    GENERATE,
    RETRIED,
    HELLO,
    LOST_400,
    LOST_400,
    LOST_400,
    sdk=SDK_RETRIES,
)
case(
    "stream_choices_that_are_no_list",
    f"stream {ODD}",
    STREAM,
    LOCAL,
    HELLO,
    stream(sse({**chunk({"content": "Hel"}), "choices": "none"}, chunk({}, finish="stop"))),
)

# -- More of how a streamed call is folded -------------------------------------------------

case(
    "stream_one_id_on_two_indexes",
    FOLDING,
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            fragment(0, "call_1", "clock", "{}"),
            fragment(1, "call_1", "lookup_order", '{"number": "A-1"}'),
            ENDED,
        )
    ),
)
case(
    "stream_call_with_an_empty_function",
    FOLDING,
    STREAM,
    LOCAL,
    TOOLS,
    stream(sse(raw_call(index=0, id="call_1", type="function", function={}), ENDED)),
)
case(
    "stream_call_name_after_its_arguments",
    FOLDING,
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            raw_call(index=0, id="call_1", type="function", function={"arguments": '{"number": '}),
            fragment(0, None, "lookup_order", '"A-1"}'),
            ENDED,
        )
    ),
)
case(
    "call_id_that_is_no_text",
    "tool_call",
    GENERATE,
    LOCAL,
    TOOLS,
    answer(
        completion(
            None,
            finish="tool_calls",
            model="local-model",
            tool_calls=[called(7, "clock", "{}")],
        )
    ),
    note="A whole answer's call id that is a number reads as its text.",
)
case(
    "stream_call_id_that_is_no_text",
    FOLDING,
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            raw_call(index=0, id=7, type="function", function={"name": "clock", "arguments": "{}"}),
            ENDED,
        )
    ),
)

# -- More of where a think tag stands ------------------------------------------------------

case(
    "stream_think_tag_across_chunks_after_text",
    f"stream {THOUGHTS}",
    STREAM,
    LOCAL,
    HELLO,
    stream(said("Hello. <th", "ink>is text</think>")),
)
for name, text in (
    ("think_block_after_a_byte_order_mark", chr(0xFEFF) + "<think>They greet me.</think>Hello."),
    ("think_block_after_a_no_break_space", chr(0xA0) + "<think>They greet me.</think>Hello."),
    ("second_think_block", "<think>One.</think>Hello.<think>Two.</think>Bye."),
):
    case(name, THOUGHTS, GENERATE, LOCAL, HELLO, answer(completion(text, model="local-model")))

# -- A response schema beside tools, and a refusal that is no text ---------------------------

ASKED_BOTH = {**TOOLS, "response_schema": CITY}
BESIDE_TOOLS = "response_schema response_schema_with_tools"
UNREAD = called("call_1", "lookup_order", "number A-1")
case(
    "response_schema_with_a_call_that_does_not_read",
    f"{BESIDE_TOOLS} tool_call_unreadable",
    GENERATE,
    OPENAI,
    ASKED_BOTH,
    answer(completion(None, finish="tool_calls", tool_calls=[UNREAD])),
)
case(
    "stream_response_schema_with_a_call_that_does_not_read",
    f"stream {BESIDE_TOOLS} tool_call_unreadable",
    STREAM,
    OPENAI,
    ASKED_BOTH,
    stream(sse(fragment(0, "call_1", "lookup_order", "number A-1"), ENDED)),
)
case(
    "response_schema_with_tools_and_an_answer_that_does_not_fit",
    f"{BESIDE_TOOLS} response_schema_mismatch",
    GENERATE,
    OPENAI,
    ASKED_BOTH,
    answer(completion("Paris is a city.")),
    note=PYTHON_WORDS,
)
case(
    "response_schema_refusal_that_is_no_text",
    "response_schema refusal",
    GENERATE,
    OPENAI,
    ASKED_A_CITY,
    answer(completion(None, refusal=True)),
)

# -- More waits, and more redirects --------------------------------------------------------

for name, kind, wait in (
    ("in_seconds_with_a_fraction", "retry_after_seconds", "1.5"),
    ("as_a_date_of_rfc_850", "retry_after_date", "Thursday, 01-Jan-26 00:00:07 GMT"),
    ("as_a_date_of_asctime", "retry_after_date", "Thu Jan  1 00:00:07 2026"),
):
    case(
        f"retry_wait_{name}",
        f"{WAIT} status_429 {kind}",
        GENERATE,
        RETRIED,
        HELLO,
        answer(LIMITED, 429, **{"retry-after": wait}),
        answer(completion()),
        sdk=SDK_RETRIES,
    )
case(
    "no_retry_where_the_answer_says_so_in_upper_case",
    f"{WAIT} status_400",
    GENERATE,
    RETRIED,
    HELLO,
    answer(BAD_REQUEST, 400, **{"x-should-retry": "TRUE"}),
    sdk=SDK_RETRIES,
)
FOLLOWED = "The openai SDK's client follows a redirect, each hop through the transport."
MOVED = {"location": "https://llm.example/moved/v1/chat/completions"}
for status in (301, 303, 308):
    case(
        f"redirect_{status}",
        "redirect",
        GENERATE,
        LOCAL,
        HELLO,
        Reply(status=status, headers=MOVED, text=""),
        answer(completion(model="local-model")),
        sdk=FOLLOWED,
    )
AROUND = Reply(status=307, headers={"location": "https://llm.example/v1/chat/completions"}, text="")
case(
    "redirects_past_the_hop_limit",
    "redirect",
    GENERATE,
    LOCAL,
    HELLO,
    *[AROUND] * 21,
    sdk=FOLLOWED,
)

# -- More of the request -------------------------------------------------------------------


def shown(url: str) -> dict[str, Any]:
    """A request that shows one image."""
    return {"messages": [{"role": "user", "content": [{"type": "image", "url": url}]}]}


case(
    "image_with_no_payload",
    "image_data_uri before_request",
    GENERATE,
    OPENAI,
    shown("data:image/png;base64,"),
)
case(
    "image_in_url_safe_base64",
    "image_data_uri before_request",
    GENERATE,
    OPENAI,
    shown("data:image/png;base64,iVBORw0KGgo-_w=="),
)
case(
    "thinking_budget_below_zero",
    SWITCH,
    GENERATE,
    GPT_5,
    {**HELLO, "thinking_budget": -1},
    answer(completion()),
)

# -- More of how a stream is framed --------------------------------------------------------

SAID = "".join(said("Hello."))
SPLIT = json.dumps(chunk({"content": "Hello."}, finish="stop"), separators=(",", ":"))
HEAD, _, REST = SPLIT.partition(",")
case(
    "stream_with_carriage_returns_alone",
    "stream stream_shapes",
    STREAM,
    LOCAL,
    HELLO,
    stream([SAID.replace("\n", "\r")]),
)
case(
    "stream_data_without_a_space",
    "stream stream_shapes",
    STREAM,
    LOCAL,
    HELLO,
    stream([SAID.replace("data: ", "data:")]),
)
case(
    "stream_event_of_several_data_lines",
    "stream stream_shapes",
    STREAM,
    LOCAL,
    HELLO,
    stream([f"data: {HEAD},\ndata: {REST}\n\n", "data: [DONE]\n\n"]),
)
case(
    "stream_text_after_the_end_mark",
    "stream stream_shapes",
    STREAM,
    LOCAL,
    HELLO,
    stream([*said("Hello."), *sse(chunk({"content": " More."}), done=False)]),
)
case(
    "stream_end_mark_with_more_on_its_line",
    "stream stream_shapes",
    STREAM,
    LOCAL,
    HELLO,
    stream([*sse(chunk({"content": "Hello."}), done=False), "data: [DONE] and more\n\n"]),
    note="A line that starts with the end mark ends the stream, whatever follows the mark.",
)
case(
    "stream_lost_after_the_end_mark",
    "stream stream_shapes",
    STREAM,
    LOCAL,
    HELLO,
    stream(said("Hello."), cut=LOST),
    note="Nothing that happens to the connection after the end mark fails the answer.",
)
case(
    "stream_event_with_no_data",
    "stream stream_shapes",
    STREAM,
    LOCAL,
    HELLO,
    stream(["event: ping\n\n", *said("Hello.")]),
)
case(
    "stream_comment_lines",
    "stream stream_shapes",
    STREAM,
    LOCAL,
    HELLO,
    stream([": keep-alive\n\n", *said("Hello."), ": bye\n\n"]),
)
case(
    "stream_second_choice_first",
    f"stream {ODD}",
    STREAM,
    LOCAL,
    HELLO,
    stream(
        sse(
            {
                "choices": [
                    {"index": 1, "delta": {"content": "one"}, "finish_reason": None},
                    {"index": 0, "delta": {"content": "zero"}, "finish_reason": None},
                ]
            },
            chunk({}, finish="stop"),
        )
    ),
    note="The first choice of the list is read, whatever index it names.",
)
case(
    "stream_delta_that_is_a_text",
    f"stream {ODD}",
    STREAM,
    LOCAL,
    HELLO,
    stream(
        sse(
            {"choices": [{"index": 0, "delta": "Hel", "finish_reason": None}]},
            chunk({"content": "lo."}),
            chunk({}, finish="stop"),
        )
    ),
)
case(
    "stream_finish_reason_that_is_no_text",
    f"stream {ODD}",
    STREAM,
    LOCAL,
    HELLO,
    stream(
        sse(
            chunk({"content": "Hello."}),
            {"choices": [{"index": 0, "delta": {}, "finish_reason": 3}]},
        )
    ),
)
case(
    "stream_call_arguments_that_are_a_number",
    f"stream tool_call_streamed {ODD}",
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            raw_call(
                index=0, id="call_1", type="function", function={"name": "clock", "arguments": 7}
            ),
            chunk({}, finish="tool_calls"),
        )
    ),
)
case(
    "stream_call_index_that_is_a_text",
    f"stream tool_call_streamed {ODD}",
    STREAM,
    LOCAL,
    TOOLS,
    stream(
        sse(
            raw_call(
                index="0",
                id="call_1",
                type="function",
                function={"name": "clock", "arguments": "{}"},
            ),
            chunk({}, finish="tool_calls"),
        )
    ),
)

# -- More error bodies ---------------------------------------------------------------------

case(
    "stream_that_sends_a_bare_error_text",
    "stream stream_error_event",
    STREAM,
    LOCAL,
    HELLO,
    stream(sse({"error": "the model is loading"})),
    sdk="The openai SDK raises a stream's error event with the event's message.",
)
case(
    "answer_200_that_is_an_error_with_a_status_in_words",
    "error_object",
    GENERATE,
    LOCAL,
    HELLO,
    answer({"error": {"message": "Quota exhausted.", "status": "RESOURCE_EXHAUSTED"}}),
)

# -- A model of the catalogue, behind a base URL -------------------------------------------

UNREAD_CATALOGUE = "Behind a base URL the catalogue is not read."
case(
    "model_that_takes_no_tools_behind_a_base_url",
    "tools reasoning_effort",
    GENERATE,
    {**LOCAL, "model": "gpt-6-astra", "reasoning_effort": "high"},
    TOOLS,
    answer(completion(model="gpt-6-astra")),
    note=UNREAD_CATALOGUE,
)
case(
    "model_served_on_responses_only_behind_a_base_url",
    "plain",
    GENERATE,
    {**LOCAL, "model": "gpt-5.5-pro"},
    HELLO,
    answer(completion(model="gpt-5.5-pro")),
    note=UNREAD_CATALOGUE,
)
case(
    "model_that_takes_tools_without_reasoning_behind_a_base_url",
    "tools reasoning_effort",
    GENERATE,
    {
        **LOCAL,
        "model": "gpt-5.4",
        "reasoning_effort": "high",
        "supports_reasoning_effort_with_tools": True,
    },
    TOOLS,
    answer(completion(model="gpt-5.4")),
    note=UNREAD_CATALOGUE,
)

# -- More answers in other shapes ----------------------------------------------------------

case("answer_naming_an_empty_model", ODD, GENERATE, LOCAL, HELLO, answer(completion(model="")))
case(
    "stream_of_usage_alone",
    f"stream {ODD}",
    STREAM,
    LOCAL,
    HELLO,
    stream(sse(chunk(usage=USAGE))),
)
case("answer_200_that_is_a_list", ODD, GENERATE, LOCAL, HELLO, answer([]))

# -- A base URL that holds user information -------------------------------------------------

BASIC = (
    "The openai SDK's HTTP client sends the user information of a URL as Basic credentials, "
    "in place of the key and of a configured Authorization."
)
CREDENTIALS = "base_url_credentials"
case(
    "base_url_with_user_information",
    CREDENTIALS,
    GENERATE,
    {**LOCAL, "base_url": "https://user:password@llm.example/v1"},
    HELLO,
    answer(completion()),
    sdk=BASIC,
)
case(
    "base_url_with_a_user_alone",
    CREDENTIALS,
    GENERATE,
    {**LOCAL, "base_url": "https://user@llm.example/v1"},
    HELLO,
    answer(completion()),
    sdk=BASIC,
)
case(
    "base_url_with_user_information_and_an_authorization_header",
    f"{CREDENTIALS} default_headers",
    STREAM,
    {
        **LOCAL,
        "base_url": "https://user:pass%20word@llm.example/v1",
        "default_headers": {"Authorization": "Bearer of-a-gateway", "x-tenant": "blue"},
    },
    HELLO,
    stream(said("Hello.")),
    sdk=BASIC,
)
case(
    "list_models_with_user_information",
    f"{CREDENTIALS} listing",
    "list_models",
    {**LOCAL, "base_url": "https://user:password@llm.example/v1"},
    {},
    answer({"object": "list", "data": [{"id": "local-model", "object": "model"}]}),
    sdk=BASIC,
)

# -- Whether the model takes images ---------------------------------------------------------

# The fixture writes what each case's provider answers for supports_vision.
# These three are the models the others do not show: one the catalogue says
# takes none, and two it does not know, read by how their names start.
for model in ("gpt-3.5-turbo", "ft:gpt-4o:example::A1", "gpt-4o-2099-01-01"):
    name = model.replace(":", "_").replace(".", "_").replace("-", "_")
    case(
        f"vision_of_{name}",
        "vision",
        GENERATE,
        {"api_key": KEY, "model": model},
        HELLO,
        answer(completion()),
    )

# -- The model listing ----------------------------------------------------------------------


def listed(*models: Any) -> dict[str, Any]:
    """The page ``GET /models`` answers: each model by its id, with what the
    endpoint says beside it."""
    entries = [
        {"id": model, "object": "model", "created": 1700000000, "owned_by": "system"}
        if isinstance(model, str)
        else model
        for model in models
    ]
    return {"object": "list", "data": entries}


# A listing reads no request: its case carries an empty one.
NOTHING: dict[str, Any] = {}
RAISED_AS_IT_IS = "list_models wraps no error: the openai SDK's is raised as it is."
case(
    "list_models",
    "listing",
    LISTING,
    OPENAI,
    NOTHING,
    answer(
        listed(
            "gpt-4o",
            "gpt-5.1",
            "text-embedding-3-small",
            "whisper-1",
            "tts-1",
            "ft:gpt-4o:example::A1",
        )
    ),
    note=(
        "The page carries ids alone. What the catalogue knows of a model is filled from "
        "it, a speech model is tagged, and a model the catalogue does not know passes "
        "through with its id."
    ),
)
case("list_models_that_lists_none", "listing", LISTING, OPENAI, NOTHING, answer(listed()))
case(
    "list_models_behind_a_base_url",
    "listing",
    LISTING,
    {**LOCAL, "default_headers": {"x-tenant": "blue"}},
    NOTHING,
    answer(listed("local-model", "gpt-4o")),
    note="The catalogue fills what it knows of a model whatever the endpoint that lists it.",
)
case(
    "list_models_status_401",
    "listing status_401",
    LISTING,
    OPENAI,
    NOTHING,
    answer(failed("Incorrect API key provided.", "invalid_request_error", "invalid_api_key"), 401),
)
case(
    "list_models_retry_that_succeeds",
    "listing retry_succeeds status_503",
    LISTING,
    RETRIED,
    NOTHING,
    answer(OVERLOADED, 503),
    answer(listed("gpt-4o")),
    sdk=SDK_RETRIES,
)
case(
    "list_models_connection_refused",
    "listing connection_refused",
    LISTING,
    OPENAI,
    NOTHING,
    REFUSED,
)
case(
    "list_models_page_without_its_models",
    "listing odd_shapes",
    LISTING,
    OPENAI,
    NOTHING,
    answer({"object": "list"}),
)
case(
    "list_models_entry_without_an_id",
    "listing odd_shapes",
    LISTING,
    OPENAI,
    NOTHING,
    answer(listed("gpt-4o", {"object": "model", "owned_by": "system"})),
)
case(
    "list_models_answered_with_a_page",
    "listing not_json",
    LISTING,
    LOCAL,
    NOTHING,
    page("<html><body>Not here</body></html>", 200),
)

# -- What the openai SDK decides, beside its retries --------------------------------------

BODY_READ_BY_THE_SDK = "The openai SDK decides how a body that is no JSON is handed on."
ALSO_DECIDED_BY_THE_SDK = {
    "extra_body": "The openai SDK merges extra_body into the body, last.",
    "extra_body_replacing_the_request_s_own": (
        "The openai SDK merges extra_body into the body, last."
    ),
    "authorization_among_the_default_headers": (
        "The openai SDK lets a default header replace the header it writes the key in."
    ),
    "base_url_with_a_trailing_slash_and_a_query": (
        "The openai SDK joins the path of a request to the base URL."
    ),
    "base_url_with_two_trailing_slashes": (
        "The openai SDK joins the path of a request to the base URL."
    ),
    "status_500_with_an_empty_body": "No body came: the message is the openai SDK's.",
    "stream_cut_before_its_end": SDK_WORDS,
    "stream_cut_inside_an_event": SDK_WORDS,
    "stream_that_sends_an_error_event": (
        "The openai SDK raises a stream's error event with the event's message."
    ),
    "stream_that_sends_an_error_naming_no_status": (
        "The openai SDK raises a stream's error event with the event's message."
    ),
    "stream_that_sends_an_overload_event": (
        "The openai SDK raises a stream's error event with the event's message."
    ),
    "answer_200_that_is_a_page": BODY_READ_BY_THE_SDK,
    "answer_200_that_is_no_json": BODY_READ_BY_THE_SDK,
    "stream_event_that_is_no_json": BODY_READ_BY_THE_SDK,
    "stream_answered_with_a_page": BODY_READ_BY_THE_SDK,
    "status_204": BODY_READ_BY_THE_SDK,
    "stream_last_event_without_its_blank_line": (
        "The openai SDK's reader hands on an event at the blank line that ends it."
    ),
    "stream_last_event_without_its_line_end": (
        "The openai SDK's reader hands on an event at the blank line that ends it."
    ),
    "stream_that_starts_with_a_byte_order_mark": (
        "The openai SDK's reader drops no byte order mark: the line it starts is no data."
    ),
    "stream_event_with_no_data": (
        "The openai SDK's reader hands on an event that holds no data, which is read as JSON."
    ),
    "list_models_status_401": RAISED_AS_IT_IS,
    "list_models_connection_refused": RAISED_AS_IT_IS,
}


# -- What is under question ----------------------------------------------------------------

# Behaviours of Python reported upstream as possible defects. A case pins
# each as Python has it today; a replay may set the case aside, or expect
# otherwise once upstream decides.
LOST_REFUSAL = (
    "A refused request whose body is lost reads as a lost connection, worth a retry, "
    "without the status the server answered."
)
TURNED_INTO_A_GET = (
    "The redirect is followed as a GET, without the body, and what it answers is read as "
    "the completion."
)
RAW_VALIDATION = (
    "A count that is no whole number escapes generate as a raw ValidationError of pydantic, "
    "no ProviderError; a stream fails with a ProviderError."
)
READ_SILENTLY = (
    "A count that is no JSON whole number is read as one without a word: a text as the "
    "number it spells, true as one."
)
POSSIBLE_DEFECTS = {
    "answer_200_that_is_no_completion": (
        "A 200 JSON body that is no completion reads as an empty answer, not as a failure."
    ),
    "base_url_with_a_trailing_slash_and_a_query": (
        "A base URL with a query: the request's path is written after the query."
    ),
    "extra_body_replacing_the_request_s_own": (
        "extra_body replaces the request's own model, temperature and messages."
    ),
    "stream_that_ends_without_an_ending": (
        "A stream that ends with no finish reason and no end mark reads as complete."
    ),
    "stream_tool_call_that_ends_without_an_ending": (
        "A stream that ends with no finish reason and no end mark hands its call as whole."
    ),
    "stream_empty_finish_reason_without_an_end_mark": (
        "A stream that ends after an empty finish reason, which says nothing, and with no "
        "end mark reads as complete."
    ),
    "stream_tool_call_empty_finish_reason_without_an_end_mark": (
        "A stream that ends after an empty finish reason, which says nothing, and with no "
        "end mark hands its call as whole."
    ),
    "stream_answered_with_a_json_error_object": (
        "A JSON error object answering a stream request is final and names no status, "
        "where generate reads the same body as the status it names."
    ),
    "key_of_the_request": "The key a request carries is not read: the configuration's is sent.",
    "redirect_302_to_another_host": (
        "A 302 to another host is followed: as a GET, without the body and without the key, "
        "and what that host answers is read as the completion."
    ),
    "usage_counts_below_zero": (
        "An input count below zero reads as zero; an output count below zero is handed on."
    ),
    "choices_that_are_no_list": (
        "Choices that are no list escape generate as a raw AttributeError, no ProviderError."
    ),
    "token_bound_of_zero_in_the_request": (
        "A token bound of zero in the request is read as none: the configuration's is sent, "
        "where a zero in the configuration is sent as it is."
    ),
    "stream_last_event_without_its_blank_line": (
        "The last event of a stream that ends without its blank line is not read."
    ),
    "stream_last_event_without_its_line_end": (
        "The last event of a stream that ends without its blank line is not read."
    ),
    "stream_status_400_whose_body_is_lost": LOST_REFUSAL,
    "stream_that_starts_with_a_byte_order_mark": (
        "The first event of a stream that starts with a byte order mark is not read."
    ),
    "status_400_whose_body_is_lost": LOST_REFUSAL,
    "status_400_whose_body_is_lost_on_every_try": (
        f"{LOST_REFUSAL} The SDK tries the refused request again."
    ),
    "stream_call_id_that_is_no_text": (
        "A streamed call whose id is a number fails the stream, where a whole answer reads "
        "the id as its text."
    ),
    "stream_call_name_after_its_arguments": (
        "A call's name that comes after its first arguments: the events of its composition "
        "leave those first arguments out, and the whole call holds them."
    ),
    "redirect_301": TURNED_INTO_A_GET,
    "redirect_303": TURNED_INTO_A_GET,
    "redirects_past_the_hop_limit": (
        "Redirects without end read, after twenty-one requests, as a lost connection, "
        "worth a retry."
    ),
    # A usage that is malformed and raises no ProviderError.
    "usage_prompt_tokens_as_text": (
        "A prompt count written as text escapes generate as a raw TypeError, no "
        "ProviderError; a stream fails with a ProviderError."
    ),
    "usage_cached_tokens_as_a_word": (
        "A cached count that is a word escapes generate as a raw TypeError, no "
        "ProviderError; a stream fails with a ProviderError."
    ),
    "usage_completion_tokens_as_a_word": RAW_VALIDATION,
    "usage_counts_with_a_fraction": RAW_VALIDATION,
    "usage_reasoning_tokens_as_a_word": RAW_VALIDATION,
    # A count that is malformed and read without a word.
    **{
        name: READ_SILENTLY
        for shape in (
            "completion_tokens_as_text",
            "counts_that_are_true",
            "cached_tokens_as_text",
            "reasoning_tokens_as_text",
        )
        for name in (f"usage_{shape}", f"stream_usage_{shape}")
    },
    "usage_cache_write_written_as_text_in_the_details": READ_SILENTLY,
}

# Points where the Rust provider is planned otherwise than Python does, which
# the maintainer decides: no defect of Python's is claimed.
TO_DECIDE = {
    "authorization_among_the_default_headers": (
        "An Authorization default header replaces the configuration's key."
    ),
    "redirect": "A redirect to the same host is followed.",
    "redirect_308": "A redirect to the same host is followed.",
}

QUESTIONED = {
    **{name: f"A possible defect: {what}" for name, what in POSSIBLE_DEFECTS.items()},
    **{name: f"A difference to decide: {what}" for name, what in TO_DECIDE.items()},
}


annotate(CASES, "sdk", ALSO_DECIDED_BY_THE_SDK)
annotate(CASES, "questioned", QUESTIONED)
