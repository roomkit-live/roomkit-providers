"""The cases ``generate_mistral_cases.py`` runs on RoomKit's Python Mistral
provider: what it sends of a request (its messages, its tools, its sampling,
its reasoning effort and its response schema), how it reads a stream (text,
``<think>`` tags, typed content chunks, calls, the finish, the usage), what it
raises for a failure, and how it lists its models.
"""

from __future__ import annotations

from typing import Any

from openai_chat_cases import CITY, CLOCK, HELLO, LOOKUP, PARIS, PNG, ROUND, TOOLS, chunk, sse
from wire_capture import (
    GENERATE,
    KEY,
    LISTING,
    LOST,
    REFUSED,
    STREAM,
    TEXT,
    TIMEOUT,
    TIMED_OUT,
    Case,
    Reply,
    answer,
    cut_every,
    page,
    registrar,
    stream,
)

CASES: list[Case] = []
case = registrar(CASES)

MISTRAL = {"api_key": KEY}
PROXIED = {**MISTRAL, "server_url": "https://llm.example"}
SMALL = {**MISTRAL, "model": "mistral-small-latest"}
HIGH = {**SMALL, "reasoning_effort": "high"}
ANSWERED_BY = "mistral-large-2512"
USAGE = {"prompt_tokens": 12, "completion_tokens": 5, "total_tokens": 17}


def asked(**members: Any) -> dict[str, Any]:
    """``HELLO``, with ``members`` of the request."""
    return {**HELLO, **members}


def piece(delta: dict[str, Any] | None = None, **members: Any) -> dict[str, Any]:
    """One chunk of a stream, as Mistral writes it."""
    return chunk(delta, model=members.pop("model", ANSWERED_BY), **members)


def streamed(*pieces: dict[str, Any], cut: str | None = None, done: bool = True) -> Any:
    return stream(sse(*pieces, done=done), cut=cut)


def said(*texts: str, finish: str = "stop", usage: dict[str, Any] | None = USAGE) -> Any:
    """A streamed answer: the role, each text, then the end with the usage."""
    pieces = [piece({"role": "assistant", "content": ""})]
    pieces += [piece({"content": each}) for each in texts]
    pieces.append(piece({"content": ""}, finish=finish, usage=usage))
    return streamed(*pieces)


def call_piece(
    name: str | None, arguments: str, *, call_id: str | None = "call_1", index: int = 0
) -> dict[str, Any]:
    """A chunk carrying one call, or a fragment of one."""
    function: dict[str, Any] = {"arguments": arguments}
    if name is not None:
        function = {"name": name, "arguments": arguments}
    entry: dict[str, Any] = {"function": function, "index": index}
    if call_id is not None:
        entry = {"id": call_id, "function": function, "index": index}
    return piece({"tool_calls": [entry]})


def thinking(*texts: str) -> dict[str, Any]:
    """A typed content chunk of reasoning."""
    return {"type": "thinking", "thinking": [{"type": "text", "text": each} for each in texts]}


def text(value: str) -> dict[str, Any]:
    return {"type": "text", "text": value}


def failed(status: int, message: str, kind: str, code: str | None = None) -> Any:
    body = {"object": "error", "message": message, "type": kind, "param": None, "code": code}
    return answer(body, status=status)


def card(model_id: str, **more: Any) -> dict[str, Any]:
    """An entry of the model listing."""
    entry: dict[str, Any] = {
        "id": model_id,
        "object": "model",
        "created": 1700000000,
        "owned_by": "mistralai",
        "capabilities": {"completion_chat": True, "function_calling": True},
        "type": "base",
    }
    entry.update(more)
    return entry


def listed(*cards: dict[str, Any]) -> Any:
    return answer({"object": "list", "data": list(cards)})


HELLO_THERE = said("Hello there.")
LOOKED_UP = streamed(
    piece({"role": "assistant", "content": ""}),
    call_piece("lookup_order", '{"number": "A-1"}'),
    piece({"content": ""}, finish="tool_calls", usage=USAGE),
)

# -- A request and its answer ---------------------------------------------------

case("plain_answer", "plain", GENERATE, MISTRAL, HELLO, HELLO_THERE)
case("plain_stream", "plain stream", STREAM, MISTRAL, HELLO, said("Hello", " there."))
case("plain_text", "plain stream", TEXT, MISTRAL, HELLO, said("Hello", " there."))
case(
    "system_prompt",
    "system_prompt",
    GENERATE,
    MISTRAL,
    asked(system_prompt="You answer in one sentence."),
    HELLO_THERE,
)
case("temperature", "sampling", GENERATE, MISTRAL, asked(temperature=0.3), HELLO_THERE)
case("max_tokens_of_the_turn", "sampling", GENERATE, MISTRAL, asked(max_tokens=64), HELLO_THERE)
case(
    "max_tokens_configured",
    "sampling",
    GENERATE,
    {**MISTRAL, "max_tokens": 4000, "model": "magistral-medium-latest"},
    HELLO,
    HELLO_THERE,
)
case(
    "conversation",
    "messages",
    GENERATE,
    MISTRAL,
    {
        "messages": [
            {"role": "user", "content": "Hello"},
            {"role": "assistant", "content": "Hello there."},
            {"role": "user", "content": [{"type": "text", "text": "Where is order A-1?"}]},
        ]
    },
    HELLO_THERE,
)
case(
    "image_inline",
    "messages images",
    GENERATE,
    MISTRAL,
    {
        "messages": [
            {
                "role": "user",
                "content": [{"type": "text", "text": "What is it?"}, {"type": "image", "url": PNG}],
            }
        ]
    },
    HELLO_THERE,
)
case(
    "image_link",
    "messages images",
    GENERATE,
    MISTRAL,
    {
        "messages": [
            {
                "role": "user",
                "content": [{"type": "image", "url": "https://images.example/cat.jpg"}],
            }
        ]
    },
    HELLO_THERE,
)
case("answer_of_another_model", "plain", GENERATE, MISTRAL, HELLO, said("Hello there."))
case(
    "answer_without_a_model",
    "plain",
    GENERATE,
    MISTRAL,
    HELLO,
    streamed(piece({"content": "Hi."}, finish="stop", model="")),
)
case(
    "server_url",
    "server_url",
    GENERATE,
    PROXIED,
    HELLO,
    HELLO_THERE,
)
case(
    "server_url_with_a_path",
    "server_url",
    GENERATE,
    {**MISTRAL, "server_url": "https://gateway.example/mistral/"},
    HELLO,
    HELLO_THERE,
)
case(
    "split_reads",
    "stream",
    STREAM,
    MISTRAL,
    HELLO,
    stream(cut_every(sse(piece({"content": "Hello there."}, finish="stop")), 7)),
)
case(
    "unicode",
    "stream",
    GENERATE,
    MISTRAL,
    HELLO,
    stream(sse(piece({"content": "café € \U0001f600"}, finish="stop"), escaped=False)),
)

# -- Reasoning ------------------------------------------------------------------

case("effort_configured", "reasoning", GENERATE, HIGH, HELLO, HELLO_THERE)
case("effort_of_the_turn", "reasoning", GENERATE, HIGH, asked(reasoning_effort="low"), HELLO_THERE)
case("effort_of_the_turn_alone", "reasoning", GENERATE, SMALL, asked(reasoning_effort="medium"), HELLO_THERE)
case("effort_none", "reasoning", GENERATE, SMALL, asked(reasoning_effort="none"), HELLO_THERE)
case("budget_zero", "reasoning", GENERATE, HIGH, asked(thinking_budget=0), HELLO_THERE)
case("budget_without_effort", "reasoning", GENERATE, SMALL, asked(thinking_budget=2048), HELLO_THERE)
case("budget_with_effort", "reasoning", GENERATE, HIGH, asked(thinking_budget=2048), HELLO_THERE)
case(
    "budget_and_effort_of_the_turn",
    "reasoning",
    GENERATE,
    SMALL,
    asked(thinking_budget=2048, reasoning_effort="low"),
    HELLO_THERE,
)
case("thinking_disabled", "reasoning", GENERATE, HIGH, asked(enable_thinking=False), HELLO_THERE)
case("thinking_enabled", "reasoning", GENERATE, SMALL, asked(enable_thinking=True), HELLO_THERE)
case(
    "thinking_chunks",
    "reasoning stream",
    STREAM,
    HIGH,
    HELLO,
    streamed(
        piece({"role": "assistant", "content": [thinking("Let me ", "think.")]}),
        piece({"content": [thinking("Done."), text("Hello")]}),
        piece({"content": [text(" there."), {"type": "reference", "reference_ids": [1]}]}),
        piece({"content": ""}, finish="stop", usage=USAGE),
    ),
)
case(
    "thinking_chunks_whole",
    "reasoning",
    GENERATE,
    HIGH,
    HELLO,
    streamed(
        piece({"content": [thinking("Weighing it."), text("Hello.")]}),
        piece({"content": ""}, finish="stop", usage=USAGE),
    ),
)
case(
    "think_tags",
    "reasoning stream",
    STREAM,
    MISTRAL,
    HELLO,
    said("<think>Weighing", " it.</think>", "Hello."),
)
case(
    "think_tag_split",
    "reasoning stream",
    STREAM,
    MISTRAL,
    HELLO,
    said("<thi", "nk>Weighing.</th", "ink>Hello."),
)
case(
    "think_tag_unclosed",
    "reasoning",
    GENERATE,
    MISTRAL,
    HELLO,
    said("<think>Weighing and", " never closing"),
)
case("text_stream_leaves_thinking_out", "reasoning stream", TEXT, MISTRAL, HELLO, said("<think>A.</think>B"))

# -- Tools and calls --------------------------------------------------------------

case("tools_declared", "tools", GENERATE, MISTRAL, TOOLS, HELLO_THERE)
case("tool_call", "tools calls", GENERATE, MISTRAL, TOOLS, LOOKED_UP)
case("tool_call_stream", "tools calls stream", STREAM, MISTRAL, TOOLS, LOOKED_UP)
case(
    "tool_call_in_fragments",
    "tools calls stream",
    STREAM,
    MISTRAL,
    TOOLS,
    streamed(
        call_piece("lookup_order", '{"num'),
        call_piece(None, 'ber": "A-1"}', call_id=None),
        piece({"content": ""}, finish="tool_calls", usage=USAGE),
    ),
)
case(
    "tool_call_id_null",
    "tools calls",
    GENERATE,
    MISTRAL,
    TOOLS,
    streamed(
        call_piece("lookup_order", '{"number": "A-1"}', call_id="null"),
        piece({"content": ""}, finish="tool_calls"),
    ),
)
case(
    "tool_call_without_id",
    "tools calls",
    GENERATE,
    MISTRAL,
    TOOLS,
    streamed(
        call_piece("lookup_order", '{"number": "A-1"}', call_id=None),
        piece({"content": ""}, finish="tool_calls"),
    ),
)
case(
    "two_tool_calls",
    "tools calls",
    GENERATE,
    MISTRAL,
    TOOLS,
    streamed(
        piece({"content": "Let me look."}),
        call_piece("lookup_order", '{"number": "A-1"}'),
        call_piece("clock", "{}", call_id="call_2", index=1),
        piece({"content": ""}, finish="tool_calls", usage=USAGE),
    ),
)
case(
    "tool_call_arguments_as_object",
    "tools calls",
    GENERATE,
    MISTRAL,
    TOOLS,
    streamed(
        piece({"tool_calls": [{"id": "call_1", "function": {"name": "lookup_order", "arguments": {"number": "A-1"}}, "index": 0}]}),
        piece({"content": ""}, finish="tool_calls"),
    ),
)
case(
    "tool_call_arguments_unread",
    "tools calls",
    GENERATE,
    MISTRAL,
    TOOLS,
    streamed(
        call_piece("lookup_order", "number A-1"),
        piece({"content": ""}, finish="tool_calls"),
    ),
)
case(
    "tool_call_cut_by_length",
    "tools calls finish",
    GENERATE,
    MISTRAL,
    TOOLS,
    streamed(
        call_piece("lookup_order", '{"number": "A'),
        piece({"content": ""}, finish="length"),
    ),
)
case("tool_round", "tools messages", GENERATE, MISTRAL, {**TOOLS, "messages": ROUND}, HELLO_THERE)
case(
    "tool_result_with_an_image",
    "tools messages images",
    GENERATE,
    MISTRAL,
    {
        **TOOLS,
        "messages": [
            *ROUND[:2],
            {
                "role": "tool",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_call_id": "call_1",
                        "name": "lookup_order",
                        "result": [{"type": "text", "text": "Shipped."}, {"type": "image", "url": PNG}],
                    },
                    {"type": "tool_result", "tool_call_id": "call_2", "name": "clock", "result": "12:00"},
                ],
            },
        ],
    },
    HELLO_THERE,
)
case(
    "tool_name_with_a_dot",
    "tools names",
    GENERATE,
    MISTRAL,
    asked(tools=[{**LOOKUP, "name": "orders.lookup"}]),
    HELLO_THERE,
)
case(
    "tool_name_refused",
    "tools names",
    GENERATE,
    MISTRAL,
    asked(tools=[{**LOOKUP, "name": "orders:lookup"}, CLOCK]),
)
case(
    "tool_name_behind_a_server_url",
    "tools names server_url",
    GENERATE,
    PROXIED,
    asked(tools=[{**LOOKUP, "name": "orders:lookup"}]),
    HELLO_THERE,
)
case(
    "tool_name_on_mistral_s_url",
    "tools names server_url",
    GENERATE,
    {**MISTRAL, "server_url": "https://api.mistral.ai"},
    asked(tools=[{**LOOKUP, "name": "orders:lookup"}]),
)

# -- Response schema --------------------------------------------------------------

case("response_schema", "response_schema", GENERATE, MISTRAL, asked(response_schema=CITY), said(PARIS))
case(
    "response_schema_stream",
    "response_schema stream",
    STREAM,
    MISTRAL,
    asked(response_schema=CITY),
    said('{"city": ', '"Paris"}'),
)
case(
    "response_schema_answered_otherwise",
    "response_schema",
    GENERATE,
    MISTRAL,
    asked(response_schema=CITY),
    said("Paris."),
)
case(
    "response_schema_missing_a_member",
    "response_schema",
    GENERATE,
    MISTRAL,
    asked(response_schema=CITY),
    said("{}"),
)
case(
    "response_schema_with_tools",
    "response_schema tools",
    GENERATE,
    MISTRAL,
    {**TOOLS, "response_schema": CITY},
)

# -- How a turn ends --------------------------------------------------------------

for reason in ("stop", "length", "model_length", "error", "content_filter", "unheard_of"):
    case(f"finish_{reason}", "finish", GENERATE, MISTRAL, HELLO, said("Hello.", finish=reason))
case(
    "finish_missing",
    "finish",
    GENERATE,
    MISTRAL,
    HELLO,
    streamed(piece({"content": "Hello."})),
)

# -- Usage ------------------------------------------------------------------------

case(
    "usage_with_cache_hits",
    "usage",
    GENERATE,
    MISTRAL,
    HELLO,
    said(
        "Hello.",
        usage={**USAGE, "prompt_tokens_details": {"cached_tokens": 8}},
    ),
)
case(
    "usage_all_cached",
    "usage",
    GENERATE,
    MISTRAL,
    HELLO,
    said(
        "Hello.",
        usage={**USAGE, "prompt_tokens_details": {"cached_tokens": 40}},
    ),
)
case(
    "usage_on_a_chunk_of_its_own",
    "usage",
    STREAM,
    MISTRAL,
    HELLO,
    streamed(
        piece({"content": "Hello."}, finish="stop"),
        chunk(usage=USAGE, model=ANSWERED_BY),
    ),
)
case("usage_missing", "usage", GENERATE, MISTRAL, HELLO, said("Hello.", usage=None))

# -- Streams that end otherwise -----------------------------------------------------

case("stream_empty", "stream", GENERATE, MISTRAL, HELLO, streamed())
case("stream_without_end_mark", "stream", GENERATE, MISTRAL, HELLO, streamed(piece({"content": "Hi."}, finish="stop"), done=False))
case(
    "stream_cut",
    "stream failure",
    STREAM,
    MISTRAL,
    HELLO,
    streamed(piece({"content": "Hel"}), cut=LOST, done=False),
)
case(
    "stream_timed_out",
    "stream failure",
    STREAM,
    MISTRAL,
    HELLO,
    streamed(piece({"content": "Hel"}), cut=TIMEOUT, done=False),
)
case(
    "stream_malformed_event",
    "stream failure",
    STREAM,
    MISTRAL,
    HELLO,
    stream(["data: {\"id\":\"cmpl-1\",\"choi\n\n", "data: [DONE]\n\n"]),
)
case(
    "stream_event_of_another_shape",
    "stream failure",
    STREAM,
    MISTRAL,
    HELLO,
    stream(['data: {"id":"cmpl-1"}\n\n', "data: [DONE]\n\n"]),
)

# -- Failures ---------------------------------------------------------------------

case(
    "bad_request",
    "failure",
    GENERATE,
    MISTRAL,
    HELLO,
    failed(400, "Invalid model: mistral-nope", "invalid_model", "1500"),
)
case(
    "unauthorized",
    "failure",
    GENERATE,
    MISTRAL,
    HELLO,
    answer({"message": "Unauthorized", "request_id": "req-1"}, status=401),
)
case(
    "validation_failed",
    "failure",
    GENERATE,
    MISTRAL,
    HELLO,
    answer(
        {"detail": [{"loc": ["body", "messages"], "msg": "Field required", "type": "missing"}]},
        status=422,
    ),
)
case(
    "rate_limited",
    "failure",
    GENERATE,
    MISTRAL,
    HELLO,
    answer({"message": "Requests rate limit exceeded"}, status=429, **{"retry-after": "3"}),
)
case(
    "context_overflow",
    "failure",
    GENERATE,
    MISTRAL,
    HELLO,
    failed(
        400,
        "Prompt contains 140000 tokens, too large for model with 131072 maximum context length",
        "invalid_request_error",
        "3051",
    ),
)
case("server_error", "failure", GENERATE, MISTRAL, HELLO, failed(500, "Internal error", "internal_error"))
case("unavailable", "failure", GENERATE, MISTRAL, HELLO, failed(503, "Service unavailable", "unavailable"))
case(
    "bad_gateway_page",
    "failure",
    GENERATE,
    MISTRAL,
    HELLO,
    page("<html><body>502 Bad Gateway</body></html>", 502),
)
case("refused_connection", "failure", GENERATE, MISTRAL, HELLO, REFUSED)
case("timed_out", "failure", GENERATE, MISTRAL, HELLO, TIMED_OUT)
case("stream_failure", "failure stream", STREAM, MISTRAL, HELLO, failed(500, "Internal error", "internal_error"))
case("text_failure", "failure stream", TEXT, MISTRAL, HELLO, failed(400, "Bad", "invalid_request_error"))

# -- The listing ------------------------------------------------------------------

case(
    "models_listed",
    "listing",
    LISTING,
    MISTRAL,
    HELLO,
    listed(
        card("mistral-large-latest", max_context_length=131072, name="Mistral Large"),
        card("ministral-8b-2410"),
        card("ft:open-mistral-7b:acme", type="fine-tuned", job="job-1", root="open-mistral-7b", archived=False),
    ),
)
case("models_listed_none", "listing", LISTING, MISTRAL, HELLO, listed())
case("models_listed_behind_a_server_url", "listing server_url", LISTING, PROXIED, HELLO, listed(card("local-model")))
case(
    "models_listing_failed",
    "listing failure",
    LISTING,
    MISTRAL,
    HELLO,
    answer({"message": "Unauthorized", "request_id": "req-1"}, status=401),
)

# -- What the SDK refuses of a chunk ------------------------------------------------
# The mistralai SDK reads each event into its models before RoomKit sees it:
# a chunk that lacks what they require, or holds what they do not take, fails
# the stream. A provider without that SDK checks the same members.


def raw(*events: str) -> Any:
    """Events written as given, each one read."""
    return stream([f"data: {event}\n\n" for event in events] + ["data: [DONE]\n\n"])


CHOICE = '"choices":[{"index":0,"delta":{"content":"Hi."},"finish_reason":"stop"}]'
case("chunk_without_an_id", "stream sdk", STREAM, MISTRAL, HELLO, raw('{"model":"m",' + CHOICE + "}"))
case("chunk_id_a_number", "stream sdk", STREAM, MISTRAL, HELLO, raw('{"id":7,"model":"m",' + CHOICE + "}"))
case("chunk_without_choices", "stream sdk", STREAM, MISTRAL, HELLO, raw('{"id":"c","model":"m"}'))
case(
    "choice_without_a_finish_reason",
    "stream sdk",
    STREAM,
    MISTRAL,
    HELLO,
    raw('{"id":"c","model":"m","choices":[{"index":0,"delta":{"content":"Hi."}}]}'),
)
case(
    "choice_without_a_delta",
    "stream sdk",
    STREAM,
    MISTRAL,
    HELLO,
    raw('{"id":"c","model":"m","choices":[{"index":0,"finish_reason":"stop"}]}'),
)
case(
    "choice_without_an_index",
    "stream sdk",
    STREAM,
    MISTRAL,
    HELLO,
    raw('{"id":"c","model":"m","choices":[{"delta":{"content":"Hi."},"finish_reason":"stop"}]}'),
)
case(
    "content_chunk_of_an_unknown_type",
    "stream sdk",
    STREAM,
    MISTRAL,
    HELLO,
    streamed(piece({"content": [{"type": "hologram", "data": "x"}]}, finish="stop")),
)
case(
    "think_chunk_without_its_thinking",
    "stream sdk",
    STREAM,
    MISTRAL,
    HELLO,
    streamed(piece({"content": [{"type": "thinking"}]}, finish="stop")),
)
case(
    "text_chunk_without_its_text",
    "stream sdk",
    STREAM,
    MISTRAL,
    HELLO,
    streamed(piece({"content": [{"type": "text"}]}, finish="stop")),
)
case(
    "content_a_number",
    "stream sdk",
    STREAM,
    MISTRAL,
    HELLO,
    streamed(piece({"content": 42}, finish="stop")),
)
case(
    "call_without_its_arguments",
    "tools calls sdk",
    GENERATE,
    MISTRAL,
    TOOLS,
    streamed(
        piece({"tool_calls": [{"id": "call_1", "function": {"name": "lookup_order"}, "index": 0}]}),
        piece({"content": ""}, finish="tool_calls"),
    ),
)
case(
    "call_without_a_function",
    "tools calls sdk",
    GENERATE,
    MISTRAL,
    TOOLS,
    streamed(piece({"tool_calls": [{"id": "call_1", "index": 0}]}), piece({"content": ""}, finish="tool_calls")),
)
case(
    "call_without_an_index",
    "tools calls sdk",
    GENERATE,
    MISTRAL,
    TOOLS,
    streamed(
        piece({"tool_calls": [{"id": "call_1", "function": {"name": "clock", "arguments": "{}"}}]}),
        piece({"content": ""}, finish="tool_calls"),
    ),
)
case(
    "usage_counts_null",
    "usage sdk",
    GENERATE,
    MISTRAL,
    HELLO,
    said("Hello.", usage={"prompt_tokens": None, "completion_tokens": None, "total_tokens": None}),
)
case(
    "usage_without_counts",
    "usage sdk",
    GENERATE,
    MISTRAL,
    HELLO,
    said("Hello.", usage={}),
)
case(
    "finish_reason_a_number",
    "finish sdk",
    GENERATE,
    MISTRAL,
    HELLO,
    raw('{"id":"c","model":"m","choices":[{"index":0,"delta":{"content":"Hi."},"finish_reason":3}]}'),
)

# -- More of what is sent ------------------------------------------------------------

case(
    "round_without_text",
    "tools messages",
    GENERATE,
    MISTRAL,
    {
        **TOOLS,
        "messages": [
            {"role": "user", "content": "Where is order A-1?"},
            {
                "role": "assistant",
                "content": [
                    {"type": "tool_call", "id": "call_1", "name": "lookup_order", "arguments": {"number": "A-1"}}
                ],
            },
            {
                "role": "tool",
                "content": [
                    {"type": "tool_result", "tool_call_id": "call_1", "name": "lookup_order", "result": "Shipped."}
                ],
            },
        ],
    },
    HELLO_THERE,
)
case(
    "tool_name_on_mistral_s_url_with_a_slash",
    "tools names server_url",
    GENERATE,
    {**MISTRAL, "server_url": "https://api.mistral.ai/"},
    asked(tools=[{**LOOKUP, "name": "orders:lookup"}]),
)
case(
    "image_of_an_assistant",
    "messages images",
    GENERATE,
    MISTRAL,
    {
        "messages": [
            {"role": "user", "content": "Draw a cat."},
            {"role": "assistant", "content": [{"type": "text", "text": "Here."}, {"type": "image", "url": PNG}]},
            {"role": "user", "content": "Thanks."},
        ]
    },
    HELLO_THERE,
)

# -- An answer that is no stream ------------------------------------------------------
# The SDK reads a 200 as a stream only when it says it is one.

case("answer_a_json_body", "stream failure sdk", GENERATE, MISTRAL, HELLO, answer({"id": "c", "choices": []}))
case("answer_a_page", "stream failure sdk", GENERATE, MISTRAL, HELLO, page("<html>Hi</html>", 200))
case(
    "answer_a_stream_with_a_charset",
    "stream sdk",
    GENERATE,
    MISTRAL,
    HELLO,
    Reply(
        status=200,
        headers={"content-type": "text/event-stream; charset=utf-8"},
        chunks=sse(piece({"content": "Hi."}, finish="stop")),
    ),
)

case("empty_key", "config", GENERATE, {**MISTRAL, "api_key": ""}, HELLO, HELLO_THERE)

# -- What the mutation sample asked for -----------------------------------------------

case(
    "content_part_without_a_type",
    "stream sdk",
    STREAM,
    MISTRAL,
    HELLO,
    streamed(piece({"content": [{"text": "Hi."}]}, finish="stop")),
)
case(
    "content_part_no_object",
    "stream sdk",
    STREAM,
    MISTRAL,
    HELLO,
    streamed(piece({"content": ["Hi."]}, finish="stop")),
)
case(
    "two_choices",
    "stream",
    STREAM,
    MISTRAL,
    HELLO,
    raw(
        '{"id":"c","model":"m","choices":[{"index":0,"delta":{"content":"First."},"finish_reason":null},'
        '{"index":1,"delta":{"content":"Second."},"finish_reason":null}]}',
        '{"id":"c","model":"m","choices":[{"index":0,"delta":{"content":""},"finish_reason":"stop"}]}',
    ),
)
case(
    "tool_call_arguments_as_object_stream",
    "tools calls stream",
    STREAM,
    MISTRAL,
    TOOLS,
    streamed(
        piece({"tool_calls": [{"id": "call_1", "function": {"name": "lookup_order", "arguments": {"number": "A-1"}}, "index": 0}]}),
        piece({"content": ""}, finish="tool_calls"),
    ),
)
case(
    "events_under_another_type",
    "stream failure sdk",
    GENERATE,
    MISTRAL,
    HELLO,
    Reply(
        status=200,
        headers={"content-type": "application/json"},
        chunks=sse(piece({"content": "Hi."}, finish="stop")),
    ),
)
case(
    "events_after_the_end_mark",
    "stream",
    STREAM,
    MISTRAL,
    HELLO,
    stream(
        [
            *sse(piece({"content": "Hi."}, finish="stop")),
            *sse(piece({"content": " More."}, finish="stop"), done=False),
        ]
    ),
)
