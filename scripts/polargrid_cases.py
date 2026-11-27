"""The cases ``generate_polargrid_cases.py`` runs on RoomKit's Python PolarGrid
provider: where it sends a request (a pinned region, or the edge the
autorouter picks), what it sends of a request (its messages, its tools, its
sampling, its token bound, its thinking switch and its response schema), how
it reads a whole answer and a stream (text, ``<think>`` tags, calls, the
finish, the usage), what it raises for a failure, how it tries a request
again, and how it lists its models.
"""

from __future__ import annotations

import json
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
    TIMED_OUT,
    TIMEOUT,
    Case,
    Reply,
    answer,
    page,
    registrar,
    stream,
)

CASES: list[Case] = []
case = registrar(CASES)

PINNED = {"api_key": KEY, "region": "yul-01"}
ROUTED = {"api_key": KEY}
PILOT = {**PINNED, "model": "qwen-3.6-35b-a3b"}
ANSWERED_BY = "qwen-3.8-27b"
USAGE = {"prompt_tokens": 12, "completion_tokens": 5, "total_tokens": 17}


def asked(**members: Any) -> dict[str, Any]:
    """``HELLO``, with ``members`` of the request."""
    return {**HELLO, **members}


def completion(
    content: str | None = "Hello there.",
    *,
    finish: str | None = "stop",
    usage: dict[str, Any] | None = USAGE,
    model: str = ANSWERED_BY,
    **message: Any,
) -> dict[str, Any]:
    said: dict[str, Any] = {"role": "assistant", "content": content, **message}
    body: dict[str, Any] = {"id": "chatcmpl-1", "object": "chat.completion", "created": 1700000000}
    body["model"] = model
    body["choices"] = [{"index": 0, "message": said, "finish_reason": finish}]
    if usage is not None:
        body["usage"] = usage
    return body


def whole(content: str | None = "Hello there.", **members: Any) -> Reply:
    return answer(completion(content, **members))


def piece(delta: dict[str, Any] | None = None, **members: Any) -> dict[str, Any]:
    return chunk(delta, model=members.pop("model", ANSWERED_BY), **members)


def streamed(*pieces: dict[str, Any], cut: str | None = None, done: bool = True) -> Reply:
    return stream(sse(*pieces, done=done), cut=cut)


def said(*texts: str, finish: str = "stop", usage: dict[str, Any] | None = USAGE) -> Reply:
    """A streamed answer: the role, each text, the end, then the usage."""
    pieces = [piece({"role": "assistant", "content": ""})]
    pieces += [piece({"content": each}) for each in texts]
    pieces.append(piece({}, finish=finish))
    if usage is not None:
        pieces.append(chunk(usage=usage, model=ANSWERED_BY))
    return streamed(*pieces)


def called(call_id: str | None, name: str, arguments: str) -> dict[str, Any]:
    call: dict[str, Any] = {"type": "function", "function": {"name": name, "arguments": arguments}}
    return call if call_id is None else {"id": call_id, **call}


def fragment(index: int, call_id: str | None, name: str | None, arguments: str) -> dict[str, Any]:
    function = {"arguments": arguments} if name is None else {"name": name, "arguments": arguments}
    call: dict[str, Any] = {"index": index, "function": function}
    if call_id is not None:
        call = {"index": index, "id": call_id, "type": "function", "function": function}
    return piece({"tool_calls": [call]})


def failed(status: int, message: str, code: str = "error") -> Reply:
    return answer({"message": message, "code": code}, status=status)


def routed(region: str = "yto-01", name: str = "Toronto", **more: Any) -> Reply:
    """What the autorouter answers: the edge it picks."""
    body = {"region": region, "endpoint": f"https://api.{region}.edge.polargrid.ai", "name": name}
    body.update(more)
    return answer(body)


def listed(*ids: str) -> Reply:
    models = [
        {"id": model, "object": "model", "created": 1700000000, "owned_by": "polargrid", "root": model}
        for model in ids
    ]
    return answer({"object": "list", "data": models})


HELLO_THERE = whole()
LONG = "é" * 9000

# -- Where a request goes ---------------------------------------------------------

case("pinned_region", "region", GENERATE, PINNED, HELLO, HELLO_THERE)
case("pinned_by_name", "region", GENERATE, {**ROUTED, "region": "toronto"}, HELLO, HELLO_THERE)
case("pinned_in_capitals", "region", GENERATE, {**ROUTED, "region": "SFO"}, HELLO, HELLO_THERE)
case("routed", "region routing", GENERATE, ROUTED, HELLO, routed(ttl=3600), HELLO_THERE)
case("routed_for_another_model", "region routing", GENERATE, {**ROUTED, "model": "qwen-3.6-35b-a3b"}, HELLO, routed("yul-02", "Montreal 02"), HELLO_THERE)
case("routed_without_a_pick", "region routing sdk", GENERATE, ROUTED, HELLO, failed(404, "no edge serves the model"), HELLO_THERE)
case("routed_router_down", "region routing sdk", GENERATE, ROUTED, HELLO, REFUSED, HELLO_THERE)
case("routed_router_page", "region routing sdk", GENERATE, ROUTED, HELLO, page("<html>Hi</html>", 200), HELLO_THERE)
case("routed_pick_incomplete", "region routing sdk", GENERATE, ROUTED, HELLO, answer({"region": "yto-01"}), HELLO_THERE)
case("routed_stream", "region routing stream", STREAM, ROUTED, HELLO, routed(), said("Hello", " there."))
case("routed_listing", "region routing listing", LISTING, ROUTED, HELLO, routed(), listed("qwen-3.8-27b"))

# -- A request and its answer ---------------------------------------------------------

case("plain_stream", "plain stream", STREAM, PINNED, HELLO, said("Hello", " there."))
case("plain_text", "plain stream", TEXT, PINNED, HELLO, said("Hello", " there."))
case("system_prompt", "messages", GENERATE, PINNED, asked(system_prompt="You answer in one sentence."), HELLO_THERE)
case(
    "conversation",
    "messages",
    GENERATE,
    PINNED,
    {
        "messages": [
            {"role": "user", "content": "Hello"},
            {"role": "assistant", "content": [{"type": "text", "text": "Hello there."}]},
            {"role": "user", "content": [{"type": "text", "text": "Where is "}, {"type": "text", "text": "A-1?"}]},
        ]
    },
    HELLO_THERE,
)
case(
    "image_inline",
    "messages images",
    GENERATE,
    PILOT,
    {"messages": [{"role": "user", "content": [{"type": "text", "text": "What is it?"}, {"type": "image", "url": PNG}]}]},
    HELLO_THERE,
)
case("temperature", "sampling", GENERATE, PINNED, asked(temperature=0.3), HELLO_THERE)
case("temperature_zero", "sampling", GENERATE, PINNED, asked(temperature=0.0), HELLO_THERE)
case("top_p_zero", "sampling", GENERATE, {**PINNED, "top_p": 0.0}, HELLO, HELLO_THERE)
case("temperature_out_of_range", "sampling sdk", GENERATE, PINNED, asked(temperature=2.5))
case("top_p_out_of_range", "sampling sdk", GENERATE, {**PINNED, "top_p": 1.5}, HELLO)
case("max_tokens_of_the_turn", "max_tokens", GENERATE, PINNED, asked(max_tokens=64), HELLO_THERE)
case("max_tokens_configured", "max_tokens", GENERATE, {**PINNED, "max_tokens": 512}, HELLO, HELLO_THERE)
case("max_tokens_of_the_turn_zero", "max_tokens", GENERATE, {**PINNED, "max_tokens": 512}, asked(max_tokens=0), HELLO_THERE)
case("max_tokens_past_the_sdk_s_bound", "max_tokens", GENERATE, PINNED, asked(max_tokens=9000), HELLO_THERE)
case("max_tokens_from_a_large_window", "max_tokens", GENERATE, PINNED, HELLO, HELLO_THERE)
case(
    "max_tokens_from_a_small_window",
    "max_tokens",
    GENERATE,
    PILOT,
    {"messages": [{"role": "user", "content": LONG}], "tools": [LOOKUP]},
    HELLO_THERE,
)
case(
    "max_tokens_from_a_window_filled",
    "max_tokens",
    GENERATE,
    PILOT,
    {"messages": [{"role": "user", "content": "x" * 30000}]},
    HELLO_THERE,
)
case("max_tokens_of_a_model_unknown", "max_tokens", GENERATE, {**PINNED, "model": "qwen-9"}, HELLO, HELLO_THERE)

# -- Thinking -------------------------------------------------------------------------

case("thinking_configured_on", "thinking", GENERATE, {**PINNED, "thinking": True}, HELLO, whole("<think>Weighing.</think>Hello."))
case("thinking_configured_off", "thinking", GENERATE, {**PINNED, "thinking": False}, HELLO, HELLO_THERE)
case("thinking_of_the_turn", "thinking", GENERATE, {**PINNED, "thinking": False}, asked(enable_thinking=True), HELLO_THERE)
case("thinking_budget_zero", "thinking", GENERATE, {**PINNED, "thinking": True}, asked(thinking_budget=0), HELLO_THERE)
case("thinking_effort_none", "thinking", GENERATE, {**PINNED, "thinking": True}, asked(reasoning_effort="none"), HELLO_THERE)
case(
    "thinking_streamed",
    "thinking stream",
    STREAM,
    {**PINNED, "thinking": True},
    HELLO,
    said("<thi", "nk>Weighing</think>", "Hello."),
)
case("thinking_unclosed", "thinking", GENERATE, PINNED, HELLO, whole("<think>Weighing and never closing"))

# -- Tools and calls --------------------------------------------------------------------

case("tools_declared", "tools", GENERATE, PINNED, TOOLS, HELLO_THERE)
case(
    "tool_call",
    "tools calls",
    GENERATE,
    PINNED,
    TOOLS,
    whole(None, finish="tool_calls", tool_calls=[called("call_1", "lookup_order", '{"number": "A-1"}')]),
)
case(
    "tool_call_without_an_id",
    "tools calls",
    GENERATE,
    PINNED,
    TOOLS,
    whole(None, finish="tool_calls", tool_calls=[called(None, "lookup_order", '{"number": "A-1"}')]),
)
case(
    "tool_call_arguments_unread",
    "tools calls",
    GENERATE,
    PINNED,
    TOOLS,
    whole(None, finish="tool_calls", tool_calls=[called("call_1", "lookup_order", "number A-1")]),
)
case(
    "tool_call_streamed",
    "tools calls stream",
    STREAM,
    PINNED,
    TOOLS,
    streamed(
        fragment(0, "call_1", "lookup_order", '{"num'),
        fragment(0, None, None, 'ber": "A-1"}'),
        fragment(1, "call_2", "clock", "{}"),
        piece({}, finish="tool_calls"),
        chunk(usage=USAGE, model=ANSWERED_BY),
    ),
)
case(
    "tool_call_streamed_cut_by_length",
    "tools calls stream finish",
    STREAM,
    PINNED,
    TOOLS,
    streamed(fragment(0, "call_1", "lookup_order", '{"number": "A'), piece({}, finish="length")),
)
case("tool_round", "tools messages", GENERATE, PINNED, {**TOOLS, "messages": ROUND}, HELLO_THERE)
case(
    "tool_result_with_an_image",
    "tools messages images",
    GENERATE,
    PILOT,
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

# -- Response schema ---------------------------------------------------------------------

case("response_schema", "response_schema", GENERATE, PINNED, asked(response_schema=CITY), whole(PARIS))
case("response_schema_stream", "response_schema stream", STREAM, PINNED, asked(response_schema=CITY), said('{"city": ', '"Paris"}'))
case("response_schema_answered_otherwise", "response_schema", GENERATE, PINNED, asked(response_schema=CITY), whole("Paris."))
case("response_schema_filtered", "response_schema finish", GENERATE, PINNED, asked(response_schema=CITY), whole("", finish="content_filter"))
case("response_schema_with_tools", "response_schema tools", GENERATE, PINNED, {**TOOLS, "response_schema": CITY})
case("response_schema_no_choice", "response_schema", GENERATE, PINNED, asked(response_schema=CITY), answer({**completion(), "choices": []}))
case(
    "response_schema_answered_with_a_call",
    "response_schema calls",
    GENERATE,
    PINNED,
    asked(response_schema=CITY),
    whole(None, finish="tool_calls", tool_calls=[called("call_1", "lookup_order", '{"number": "A-1"}')]),
)
case(
    "response_schema_stream_filtered",
    "response_schema stream finish",
    STREAM,
    PINNED,
    asked(response_schema=CITY),
    said(finish="content_filter"),
)

# -- How an answer ends --------------------------------------------------------------------

for reason in ("stop", "length", "content_filter", "unheard_of"):
    case(f"finish_{reason}", "finish", GENERATE, PINNED, HELLO, whole("Hello.", finish=reason))
case("answer_without_a_choice", "finish", GENERATE, PINNED, HELLO, answer({**completion(), "choices": []}))
case("answer_without_its_usage", "sdk", GENERATE, PINNED, HELLO, whole(usage=None))
case("stream_usage_missing", "usage stream", STREAM, PINNED, HELLO, said("Hi.", usage=None))
case("stream_usage_unread", "usage stream sdk", STREAM, PINNED, HELLO, said("Hi.", usage={"prompt_tokens": 3}))
case("stream_empty", "stream", STREAM, PINNED, HELLO, streamed())
case("stream_without_end_mark", "stream", STREAM, PINNED, HELLO, streamed(piece({"content": "Hi."}, finish="stop"), done=False))
case(
    "stream_line_no_json",
    "stream sdk",
    STREAM,
    PINNED,
    HELLO,
    stream(["data: not json\n\n", *sse(piece({"content": "Hi."}, finish="stop"))]),
)
case(
    "stream_chunk_of_another_shape",
    "stream sdk",
    STREAM,
    PINNED,
    HELLO,
    stream(['data: {"pg_metadata": {"edge": "yul-01"}}\n\n', *sse(piece({"content": "Hi."}, finish="stop"))]),
)
case(
    "stream_past_its_end_mark",
    "stream sdk",
    STREAM,
    PINNED,
    HELLO,
    stream([*sse(piece({"content": "Hi."}, finish="stop")), *sse(piece({"content": " Late."}), done=False)]),
)
case(
    "stream_chunk_of_another_object",
    "stream sdk",
    STREAM,
    PINNED,
    HELLO,
    stream(
        [
            *sse({**piece({"content": "Skipped."}), "object": "chat.completion"}, done=False),
            *sse(piece({"content": "Hi."}, finish="stop")),
        ]
    ),
)
case("stream_answered_by_another_model", "stream", STREAM, PILOT, HELLO, said("Hi."))
case(
    "stream_error_event",
    "stream failure sdk",
    STREAM,
    PINNED,
    HELLO,
    stream([*sse(piece({"content": "Hel"}), done=False), 'data: {"error": {"message": "edge overloaded"}}\n\n']),
)
case("stream_cut", "stream failure", STREAM, PINNED, HELLO, streamed(piece({"content": "Hel"}), cut=LOST, done=False))
case("stream_timed_out", "stream failure", STREAM, PINNED, HELLO, streamed(piece({"content": "Hel"}), cut=TIMEOUT, done=False))

# -- Failures -------------------------------------------------------------------------------

case("bad_request", "failure", GENERATE, PINNED, HELLO, failed(400, "invalid request", "invalid_request"))
case("unauthorized", "failure", GENERATE, PINNED, HELLO, failed(401, "invalid api key", "unauthorized"))
case("forbidden", "failure", GENERATE, PINNED, HELLO, failed(403, "forbidden", "forbidden"))
case("payment_required", "failure", GENERATE, PINNED, HELLO, failed(402, "insufficient credits", "billing"))
case("model_not_loaded", "failure", GENERATE, PINNED, HELLO, failed(404, "model qwen-3.8-27b not loaded", "model_not_loaded"))
case("rate_limited", "failure", GENERATE, PINNED, HELLO, failed(429, "too many requests", "rate_limited"))
case("server_error", "failure", GENERATE, PINNED, HELLO, failed(500, "internal error"))
case("unavailable", "failure", GENERATE, PINNED, HELLO, failed(503, "edge draining"))
case("bad_gateway_page", "failure", GENERATE, PINNED, HELLO, page("<html>502 Bad Gateway</html>", 502))
case("teapot", "failure", GENERATE, PINNED, HELLO, failed(418, "teapot"))
case("detail_instead_of_a_message", "failure", GENERATE, PINNED, HELLO, answer({"detail": "context too long"}, status=400))
case(
    "message_beside_a_detail",
    "failure",
    GENERATE,
    PINNED,
    HELLO,
    answer({"message": "invalid request", "detail": "context too long"}, status=400),
)
case("refused_connection", "failure", GENERATE, PINNED, HELLO, REFUSED)
case("timed_out", "failure", GENERATE, PINNED, HELLO, TIMED_OUT)
case("answer_no_json", "failure", GENERATE, PINNED, HELLO, page("not json", 200, "application/json"))
case("stream_failure", "failure stream", STREAM, PINNED, HELLO, failed(500, "internal error"))
case("stream_unauthorized", "failure stream", STREAM, PINNED, HELLO, failed(401, "invalid api key", "unauthorized"))
case("stream_refused_connection", "failure stream", STREAM, PINNED, HELLO, REFUSED)
case("text_failure", "failure stream", TEXT, PINNED, HELLO, failed(400, "bad"))

# -- Tries again -----------------------------------------------------------------------------

TRIES = {**PINNED, "max_retries": 2}
case("tried_again_after_a_lost_connection", "retry sdk", GENERATE, TRIES, HELLO, REFUSED, HELLO_THERE)
case("tried_again_after_a_timeout", "retry sdk", GENERATE, TRIES, HELLO, TIMED_OUT, TIMED_OUT, HELLO_THERE)
case("tried_again_until_none_is_left", "retry sdk", GENERATE, TRIES, HELLO, REFUSED, REFUSED, REFUSED)
case("not_tried_again_after_a_server_error", "retry sdk", GENERATE, TRIES, HELLO, failed(503, "edge draining"))
case("stream_not_tried_again", "retry stream sdk", STREAM, TRIES, HELLO, REFUSED)

# -- The listing --------------------------------------------------------------------------------

case(
    "models_listed",
    "listing",
    LISTING,
    PINNED,
    HELLO,
    listed("qwen-3.8-27b", "qwen-3.6-35b-a3b", "whisper-large-v3-turbo", "kokoro-82m", "some-new-tts", "unknown-model"),
)
case("models_listed_none", "listing", LISTING, PINNED, HELLO, listed())
case("models_listing_failed", "listing failure", LISTING, PINNED, HELLO, failed(401, "invalid api key", "unauthorized"))
case(
    "models_listed_of_another_shape",
    "listing sdk",
    LISTING,
    PINNED,
    HELLO,
    answer({"object": "list", "data": [{"id": "qwen-3.8-27b"}]}),
)

# Kept for the table's helpers: a call's id the server gives, a clock tool.
_ = (CLOCK, json)
