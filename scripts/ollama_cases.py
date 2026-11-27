"""The cases ``generate_ollama_cases.py`` runs on RoomKit's Python Ollama
provider, on Ollama's native API: what it sends of a request (its messages,
its tools, its options, its thinking and its format), how it reads a whole
answer and a stream of JSON lines (text, thinking, calls, the end, the
usage), what it raises for a failure, how it lists the installed models and
their capabilities, and where it sends what its host and its headers say.
"""

from __future__ import annotations

import json
from typing import Any

from openai_chat_cases import CITY, HELLO, PARIS, PNG, TOOLS
from wire_capture import (
    GENERATE,
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
)

CASES: list[Case] = []
case = registrar(CASES)

OLLAMA: dict[str, Any] = {}
THINKER = {"model": "qwen3:8b"}
LEVELLED = {"model": "gpt-oss:20b", "think": "high"}
ANSWERED_BY = "llama3.2:latest"
CREATED = "2026-01-01T00:00:00.000000Z"
NDJSON = {"content-type": "application/x-ndjson"}


def asked(**members: Any) -> dict[str, Any]:
    """``HELLO``, with ``members`` of the request."""
    return {**HELLO, **members}


def said_whole(
    content: str = "Hello there.",
    *,
    done_reason: str | None = "stop",
    counts: bool = True,
    model: str | None = ANSWERED_BY,
    **message: Any,
) -> dict[str, Any]:
    """A whole answer, as ``/api/chat`` writes one with ``stream: false``."""
    body: dict[str, Any] = {}
    if model is not None:
        body["model"] = model
    body["created_at"] = CREATED
    body["message"] = {"role": "assistant", "content": content, **message}
    body["done"] = True
    if done_reason is not None:
        body["done_reason"] = done_reason
    body["total_duration"] = 1000
    if counts:
        body["prompt_eval_count"] = 12
        body["eval_count"] = 5
    return body


def whole(content: str = "Hello there.", **members: Any) -> Reply:
    return answer(said_whole(content, **members))


def line(content: str = "", *, done: bool = False, **members: Any) -> dict[str, Any]:
    """One line of a stream."""
    message: dict[str, Any] = {"role": "assistant", "content": content}
    for name in ("thinking", "tool_calls", "images"):
        if name in members:
            message[name] = members.pop(name)
    body: dict[str, Any] = {"model": members.pop("model", ANSWERED_BY), "created_at": CREATED}
    body["message"] = message
    body["done"] = done
    body.update(members)
    return body


def end(**members: Any) -> dict[str, Any]:
    """The last line of a stream: done, its reason and its counts."""
    counted = {"prompt_eval_count": 12, "eval_count": 5}
    return line(done=True, **{"done_reason": "stop", **counted, **members})


def lines(*parts: dict[str, Any] | str, cut: str | None = None, status: int = 200) -> Reply:
    """Each part one read of the stream, a JSON line, or a text as it is."""
    chunks = [part if isinstance(part, str) else json.dumps(part) + "\n" for part in parts]
    return Reply(status=status, headers=dict(NDJSON), chunks=chunks, cut=cut)


def streamed(*texts: str, **end_members: Any) -> Reply:
    return lines(*(line(text) for text in texts), end(**end_members))


def call(name: str, arguments: Any, **more: Any) -> dict[str, Any]:
    return {**more, "function": {"name": name, "arguments": arguments}}


def failed(status: int, error: str) -> Reply:
    return answer({"error": error}, status=status)


def installed(*names: str) -> Reply:
    models = [
        {
            "name": name,
            "model": name,
            "modified_at": CREATED,
            "size": 2019393189,
            "digest": "a80c4f17acd5",
            "details": {"format": "gguf", "family": "llama", "parameter_size": "3.2B"},
        }
        for name in names
    ]
    return answer({"models": models})


def shown(*capabilities: str) -> Reply:
    """What ``/api/show`` answers of a model: ``model_info`` among what the
    SDK requires."""
    return answer(
        {
            "modelfile": "FROM x",
            "parameters": "stop <|eot|>",
            "template": "{{ .Prompt }}",
            "details": {"format": "gguf", "family": "llama"},
            "model_info": {"general.architecture": "llama"},
            "capabilities": list(capabilities),
            "modified_at": CREATED,
        }
    )


HELLO_THERE = whole()

# -- A request and its answer ---------------------------------------------------

case("plain_answer", "plain", GENERATE, OLLAMA, HELLO, HELLO_THERE)
case("plain_stream", "plain stream", STREAM, OLLAMA, HELLO, streamed("Hello", " there."))
case("plain_text", "plain stream", TEXT, OLLAMA, HELLO, streamed("Hello", " there."))
case(
    "system_prompt",
    "messages",
    GENERATE,
    OLLAMA,
    asked(system_prompt="You answer in one sentence."),
    HELLO_THERE,
)
case(
    "conversation",
    "messages",
    GENERATE,
    OLLAMA,
    {
        "messages": [
            {"role": "user", "content": "Hello"},
            {"role": "assistant", "content": [{"type": "text", "text": "Hello there."}]},
            {"role": "user", "content": [{"type": "text", "text": "Where is "}, {"type": "text", "text": "A-1?"}]},
        ]
    },
    HELLO_THERE,
)
case("temperature", "options", GENERATE, OLLAMA, asked(temperature=0.3), HELLO_THERE)
case("max_tokens_of_the_turn", "options", GENERATE, OLLAMA, asked(max_tokens=64), HELLO_THERE)
case("max_tokens_configured", "options", GENERATE, {"max_tokens": 512}, HELLO, HELLO_THERE)
case(
    "every_option",
    "options",
    GENERATE,
    {"num_ctx": 8192, "top_p": 0.9, "top_k": 40, "min_p": 0.05, "max_tokens": 256},
    asked(temperature=0.5),
    HELLO_THERE,
)
for name, keep_alive in (("text", "5m"), ("seconds", 300), ("forever", -1), ("text_of_seconds", "-1")):
    case(f"keep_alive_{name}", "options", GENERATE, {"keep_alive": keep_alive}, HELLO, HELLO_THERE)
case("another_model", "plain", GENERATE, {"model": "mistral:7b"}, HELLO, whole(model="mistral:7b"))
case("answer_without_a_model", "plain", GENERATE, OLLAMA, HELLO, whole(model=None))

# -- Thinking ---------------------------------------------------------------------

case("think_on", "thinking", GENERATE, {**THINKER, "think": True}, HELLO, whole(thinking="Weighing it."))
case("think_off", "thinking", GENERATE, {**THINKER, "think": False}, HELLO, HELLO_THERE)
case("think_level", "thinking", GENERATE, LEVELLED, HELLO, whole(thinking="Weighing."))
case("think_of_the_model", "thinking", GENERATE, THINKER, HELLO, HELLO_THERE)
case("budget_zero", "thinking", GENERATE, {**THINKER, "think": True}, asked(thinking_budget=0), HELLO_THERE)
case("budget_positive", "thinking", GENERATE, THINKER, asked(thinking_budget=2048), HELLO_THERE)
case("enable_thinking", "thinking", GENERATE, {**THINKER, "think": False}, asked(enable_thinking=True), HELLO_THERE)
case("disable_thinking", "thinking", GENERATE, LEVELLED, asked(enable_thinking=False), HELLO_THERE)
case("effort_none", "thinking", GENERATE, LEVELLED, asked(reasoning_effort="none"), HELLO_THERE)
case("effort_of_the_turn", "thinking", GENERATE, LEVELLED, asked(reasoning_effort="low"), HELLO_THERE)
case("effort_off_the_levels", "thinking", GENERATE, LEVELLED, asked(reasoning_effort="xhigh"), HELLO_THERE)
case("effort_minimal", "thinking", GENERATE, LEVELLED, asked(reasoning_effort="minimal"), HELLO_THERE)
case("effort_without_levels", "thinking", GENERATE, THINKER, asked(reasoning_effort="low"), HELLO_THERE)
case(
    "thinking_streamed",
    "thinking stream",
    STREAM,
    {**THINKER, "think": True},
    HELLO,
    lines(line(thinking="Weighing"), line(thinking=" it."), line("Hello."), end()),
)
case("text_leaves_thinking_out", "thinking stream", TEXT, THINKER, HELLO, lines(line(thinking="A."), line("B"), end()))

# -- Images -------------------------------------------------------------------------


def looked_at(url: str) -> dict[str, Any]:
    return {"messages": [{"role": "user", "content": [{"type": "text", "text": "What is it?"}, {"type": "image", "url": url}]}]}


case("image_inline", "images", GENERATE, OLLAMA, looked_at(PNG), HELLO_THERE)
case("image_jpeg_inline", "images", GENERATE, OLLAMA, looked_at("data:image/jpeg;base64,/9j/4AAQSkZJRg=="), HELLO_THERE)
case("image_as_base64", "images", GENERATE, OLLAMA, looked_at("iVBORw0KGgo="), HELLO_THERE)
case("image_a_link", "images", GENERATE, OLLAMA, looked_at("https://images.example/cat.jpg"))
case("image_a_missing_file", "images", GENERATE, OLLAMA, looked_at("no/such/picture.png"))
case("image_no_base64", "images", GENERATE, OLLAMA, looked_at("not an image at all"))
case("image_a_link_of_no_known_kind", "images", GENERATE, OLLAMA, looked_at("https://images.example/cat"), HELLO_THERE)
case("image_data_uri_broken", "images", GENERATE, OLLAMA, looked_at("data:image/png;base64,***"))
case(
    "image_of_an_assistant",
    "images messages",
    GENERATE,
    OLLAMA,
    {
        "messages": [
            {"role": "user", "content": "Draw a cat."},
            {"role": "assistant", "content": [{"type": "text", "text": "Here."}, {"type": "image", "url": PNG}]},
            {"role": "user", "content": "Thanks."},
        ]
    },
    HELLO_THERE,
)

# -- Tools and calls ----------------------------------------------------------------

LOOKED_UP = whole("", tool_calls=[call("lookup_order", {"number": "A-1"})])
ROUND = [
    {"role": "user", "content": "Where is order A-1?"},
    {
        "role": "assistant",
        "content": [
            {"type": "thinking", "thinking": "The order number is A-1."},
            {"type": "text", "text": "Let me look."},
            {"type": "tool_call", "id": "call_1", "name": "lookup_order", "arguments": {"number": "A-1"}},
            {"type": "tool_call", "id": "call_2", "name": "clock", "arguments": {}},
        ],
    },
    {
        "role": "tool",
        "content": [
            {"type": "tool_result", "tool_call_id": "call_1", "name": "lookup_order", "result": "Shipped."},
            {"type": "tool_result", "tool_call_id": "call_2", "name": "clock", "result": "12:00"},
        ],
    },
]
NESTED = {
    "name": "book",
    "description": "Book a table",
    "parameters": {
        "type": "object",
        "properties": {
            "booking": {
                "type": "object",
                "properties": {"zq_code": {"type": "string"}, "guests": {"type": "integer", "minimum": 1}},
                "required": ["zq_code"],
            },
            "when": {"anyOf": [{"type": "string"}, {"type": "null"}]},
        },
        "required": ["booking"],
    },
}

case("tools_declared", "tools", GENERATE, OLLAMA, TOOLS, HELLO_THERE)
case("tools_nested_schema", "tools", GENERATE, OLLAMA, asked(tools=[NESTED]), HELLO_THERE)
case("tool_call", "tools calls", GENERATE, OLLAMA, TOOLS, LOOKED_UP)
case(
    "tool_call_stream",
    "tools calls stream",
    STREAM,
    OLLAMA,
    TOOLS,
    lines(line(tool_calls=[call("lookup_order", {"number": "A-1"})]), end()),
)
case(
    "two_tool_calls",
    "tools calls",
    GENERATE,
    OLLAMA,
    TOOLS,
    whole("Let me look.", tool_calls=[call("lookup_order", {"number": "A-1"}), call("clock", {})]),
)
case(
    "tool_calls_across_lines",
    "tools calls stream",
    STREAM,
    OLLAMA,
    TOOLS,
    lines(
        line("Let me look."),
        line(tool_calls=[call("lookup_order", {"number": "A-1"})]),
        line(tool_calls=[call("clock", {})]),
        end(),
    ),
)
case(
    "tool_call_with_an_id",
    "tools calls",
    GENERATE,
    OLLAMA,
    TOOLS,
    whole("", tool_calls=[call("lookup_order", {"number": "A-1"}, id="call_server_1")]),
)
case(
    "tool_call_arguments_as_text",
    "tools calls",
    GENERATE,
    OLLAMA,
    TOOLS,
    whole("", tool_calls=[call("lookup_order", '{"number": "A-1"}')]),
)
case(
    "tool_call_without_a_name",
    "tools calls",
    GENERATE,
    OLLAMA,
    TOOLS,
    whole("", tool_calls=[{"function": {"arguments": {}}}]),
)
case("tool_round", "tools messages", GENERATE, OLLAMA, {**TOOLS, "messages": ROUND}, HELLO_THERE)
case(
    "tool_result_with_an_image",
    "tools messages images",
    GENERATE,
    OLLAMA,
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
    "round_without_tools_declared",
    "tools messages",
    GENERATE,
    OLLAMA,
    {"messages": ROUND},
    HELLO_THERE,
)

# -- Response schema ------------------------------------------------------------------

case("response_schema", "response_schema", GENERATE, OLLAMA, asked(response_schema=CITY), whole(PARIS))
case(
    "response_schema_stream",
    "response_schema stream",
    STREAM,
    OLLAMA,
    asked(response_schema=CITY),
    streamed('{"city": ', '"Paris"}'),
)
case("response_schema_answered_otherwise", "response_schema", GENERATE, OLLAMA, asked(response_schema=CITY), whole("Paris."))
case("response_schema_missing_a_member", "response_schema", GENERATE, OLLAMA, asked(response_schema=CITY), whole("{}"))
case("response_schema_with_tools", "response_schema tools", GENERATE, OLLAMA, {**TOOLS, "response_schema": CITY})
case(
    "response_schema_answered_by_a_call",
    "response_schema tools",
    GENERATE,
    OLLAMA,
    asked(response_schema=CITY),
    whole("", tool_calls=[call("lookup_order", {"number": "A-1"})]),
)

# -- How an answer ends -------------------------------------------------------------------

for reason in ("stop", "length", "load", "unload", "unheard_of"):
    case(f"done_{reason}", "finish", GENERATE, OLLAMA, HELLO, whole(done_reason=reason))
case("done_without_a_reason", "finish", GENERATE, OLLAMA, HELLO, whole(done_reason=None))
case("stream_done_length", "finish stream", STREAM, OLLAMA, HELLO, streamed("Hel", done_reason="length"))
case("usage_missing", "usage", GENERATE, OLLAMA, HELLO, whole(counts=False))
case("stream_usage_missing", "usage stream", STREAM, OLLAMA, HELLO, lines(line("Hi."), line(done=True, done_reason="stop")))
case("stream_without_done", "stream", STREAM, OLLAMA, HELLO, lines(line("Hi.")))
case("stream_empty", "stream", STREAM, OLLAMA, HELLO, lines())
case("stream_blank_lines", "stream", STREAM, OLLAMA, HELLO, lines("\n", line("Hi."), "\n", end()))
case(
    "stream_lines_split_across_reads",
    "stream",
    STREAM,
    OLLAMA,
    HELLO,
    lines(json.dumps(line("Hel"))[:20], json.dumps(line("Hel"))[20:] + "\n", json.dumps(end()) + "\n"),
)
case("stream_last_line_without_its_end", "stream", STREAM, OLLAMA, HELLO, lines(line("Hi."), json.dumps(end())))

# -- Failures ---------------------------------------------------------------------------

case("model_not_found", "failure", GENERATE, OLLAMA, HELLO, failed(404, "model 'llama3.2' not found"))
case("bad_request", "failure", GENERATE, OLLAMA, HELLO, failed(400, 'think value "low" is not supported for this model'))
case("unauthorized", "failure", GENERATE, {"api_key": "placeholder-not-a-key"}, HELLO, failed(401, "unauthorized"))
case("server_error", "failure", GENERATE, OLLAMA, HELLO, failed(500, "llama runner process has terminated"))
case("unavailable", "failure", GENERATE, OLLAMA, HELLO, failed(503, "server busy"))
case("bad_gateway_page", "failure", GENERATE, OLLAMA, HELLO, page("<html>502 Bad Gateway</html>", 502))
case("error_without_json", "failure", GENERATE, OLLAMA, HELLO, page("plain failure", 500, "text/plain"))
case("context_overflow", "failure", GENERATE, OLLAMA, HELLO, failed(400, "the input length exceeds the context length"))
case("refused_connection", "failure", GENERATE, OLLAMA, HELLO, REFUSED)
case("timed_out", "failure", GENERATE, OLLAMA, HELLO, TIMED_OUT)
case("answer_no_json", "failure", GENERATE, OLLAMA, HELLO, page("not json", 200, "application/json"))
case("answer_without_its_message", "failure", GENERATE, OLLAMA, HELLO, answer({"model": "m", "done": True}))
case("stream_failure", "failure stream", STREAM, OLLAMA, HELLO, failed(500, "llama runner process has terminated"))
case(
    "stream_error_line",
    "failure stream",
    STREAM,
    OLLAMA,
    HELLO,
    lines(line("Hel"), {"error": "XML syntax error on line 1: element <function> closed by </parameter>"}),
)
case("stream_cut", "failure stream", STREAM, OLLAMA, HELLO, lines(line("Hel"), cut=LOST))
case("stream_timed_out", "failure stream", STREAM, OLLAMA, HELLO, lines(line("Hel"), cut=TIMEOUT))
case("stream_line_no_json", "failure stream", STREAM, OLLAMA, HELLO, lines(line("Hel"), "not json\n"))
case("text_failure", "failure stream", TEXT, OLLAMA, HELLO, failed(400, "bad"))

# -- Hosts and headers --------------------------------------------------------------------

for name, host in (
    ("http_with_a_port", "http://ollama.example:8080"),
    ("http_without_a_port", "http://ollama.example"),
    ("https_without_a_port", "https://ollama.example"),
    ("no_scheme", "ollama.example"),
    ("no_scheme_with_a_port", "ollama.example:9000"),
    ("with_a_path", "https://ollama.example/ollama/"),
    ("in_capitals", "http://Ollama.Example:11434"),
    ("ipv6", "http://[::1]:11434"),
    ("with_credentials", "https://user:secret@ollama.example"),
):
    case(f"host_{name}", "host", GENERATE, {"host": host}, HELLO, HELLO_THERE)
case("api_key", "headers", GENERATE, {"api_key": "placeholder-not-a-key"}, HELLO, HELLO_THERE)
case("headers", "headers", GENERATE, {"headers": {"X-Proxy-Token": "t-1", "X-Team": "core"}}, HELLO, HELLO_THERE)
case(
    "api_key_over_an_authorization_header",
    "headers",
    GENERATE,
    {"api_key": "placeholder-not-a-key", "headers": {"Authorization": "Basic dXNlcjpwYXNz"}},
    HELLO,
    HELLO_THERE,
)
case(
    "authorization_header_alone",
    "headers",
    GENERATE,
    {"headers": {"Authorization": "Basic dXNlcjpwYXNz"}},
    HELLO,
    HELLO_THERE,
)

# -- The listing ----------------------------------------------------------------------------

case(
    "models_listed",
    "listing",
    LISTING,
    OLLAMA,
    HELLO,
    installed("llama3.2:latest", "qwen3:8b", "nomic-embed-text:latest"),
    shown("completion", "tools"),
    shown("completion", "tools", "thinking"),
    shown("embedding"),
)
case("models_listed_none", "listing", LISTING, OLLAMA, HELLO, installed())
case(
    "models_shown_otherwise",
    "listing",
    LISTING,
    OLLAMA,
    HELLO,
    installed("llama3.2:latest", "old:latest"),
    shown("completion"),
    failed(500, "show failed"),
)
case(
    "models_shown_without_capabilities",
    "listing",
    LISTING,
    OLLAMA,
    HELLO,
    installed("llama3.2:latest"),
    answer({"modelfile": "FROM x", "model_info": {}}),
)
case(
    "models_shown_without_what_the_sdk_requires",
    "listing sdk",
    LISTING,
    OLLAMA,
    HELLO,
    installed("llama3.2:latest"),
    answer({"modelfile": "FROM x", "capabilities": ["completion"]}),
)
case("models_listing_failed", "listing failure", LISTING, OLLAMA, HELLO, failed(500, "listing failed"))

case("keep_alive_and_format", "options", GENERATE, {"keep_alive": "5m"}, asked(response_schema=CITY), whole(PARIS))
case(
    "everything_with_tools",
    "options tools",
    GENERATE,
    {**LEVELLED, "keep_alive": 300, "num_ctx": 4096},
    {**TOOLS, "temperature": 0.2},
    HELLO_THERE,
)

# -- What the mutation sample asked for -----------------------------------------------

case(
    "round_with_an_image",
    "tools messages images",
    GENERATE,
    OLLAMA,
    {
        **TOOLS,
        "messages": [
            {"role": "user", "content": "Where is order A-1?"},
            {
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Here it is."},
                    {"type": "image", "url": PNG},
                    {"type": "tool_call", "id": "call_1", "name": "lookup_order", "arguments": {"number": "A-1"}},
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
case("answer_with_an_empty_model", "plain", GENERATE, OLLAMA, HELLO, whole(model=""))
case(
    "stream_end_without_done",
    "stream finish",
    STREAM,
    OLLAMA,
    HELLO,
    lines(
        line("Hi."),
        {"model": ANSWERED_BY, "message": {"role": "assistant", "content": ""}, "done_reason": "length", "eval_count": 5},
    ),
)
case(
    "headers_the_sdk_sets_over",
    "headers",
    GENERATE,
    {"headers": {"Accept": "text/plain", "Content-Type": "text/plain", "User-Agent": "host/1.0"}},
    HELLO,
    HELLO_THERE,
)
