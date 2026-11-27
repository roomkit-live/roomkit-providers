"""The cases ``generate_gemini_cases.py`` runs on RoomKit's Python Gemini
provider, on the Developer API: what the provider sends of a request (its
contents, its system instruction, its tools, its thinking and its response
schema), how it reads a stream (text, thoughts, calls and their signatures,
the finish, the usage, a blocked prompt), what it raises for a failure, and
how it lists its models.
"""

from __future__ import annotations

import json
from typing import Any

from openai_chat_cases import CITY, HELLO, LOOKUP, PNG, TOOLS
from wire_capture import (
    GENERATE,
    KEY,
    LISTING,
    LOST,
    STREAM,
    TEXT,
    TIMEOUT,
    Case,
    Turn,
    answer,
    page,
    registrar,
    stream,
)

CASES: list[Case] = []
case = registrar(CASES)

GEMINI = {"api_key": KEY}
ANSWERED_BY = "gemini-3.8-flash-001"
SIGNATURE = "c2lnbmF0dXJlLTE="
OTHER_SIGNATURE = "c2lnbmF0dXJlLTI="


def asked(**members: Any) -> dict[str, Any]:
    """``HELLO``, with ``members`` of the request."""
    return {**HELLO, **members}


def text(value: str) -> dict[str, Any]:
    return {"text": value}


def thought(value: str) -> dict[str, Any]:
    return {"text": value, "thought": True}


def call(name: str, args: dict[str, Any], signature: str | None = None) -> dict[str, Any]:
    part: dict[str, Any] = {"functionCall": {"name": name, "args": args}}
    if signature is not None:
        part["thoughtSignature"] = signature
    return part


def usage(
    prompt: int = 12,
    candidates: int = 5,
    total: int | None = None,
    **more: int,
) -> dict[str, Any]:
    counted: dict[str, Any] = {"promptTokenCount": prompt, "candidatesTokenCount": candidates}
    counted.update(more)
    counted["totalTokenCount"] = total if total is not None else prompt + candidates
    return counted


def piece(
    *parts: dict[str, Any],
    finish: str | None = None,
    counted: dict[str, Any] | None = None,
    block: str | None = None,
    model: str | None = ANSWERED_BY,
) -> dict[str, Any]:
    """One chunk of a stream: a candidate holding ``parts``, its finish, the
    usage so far, a prompt's block."""
    body: dict[str, Any] = {}
    if parts or finish:
        candidate: dict[str, Any] = {"content": {"role": "model", "parts": list(parts)}}
        if finish:
            candidate["finishReason"] = finish
        candidate["index"] = 0
        body["candidates"] = [candidate]
    if counted is not None:
        body["usageMetadata"] = counted
    if block is not None:
        body["promptFeedback"] = {"blockReason": block}
    if model is not None:
        body["modelVersion"] = model
    body["responseId"] = "response-1"
    return body


def events(*pieces: dict[str, Any]) -> list[str]:
    """Each piece as one read of the stream, as the Developer API writes it."""
    return [f"data: {json.dumps(each, separators=(',', ':'))}\r\n\r\n" for each in pieces]


def streamed(*pieces: dict[str, Any], cut: str | None = None) -> Any:
    return stream(events(*pieces), cut=cut)


def said(*texts: str, finish: str = "STOP") -> Any:
    """A streamed answer: each text a chunk, the last with the finish and the
    usage."""
    pieces = [piece(text(each)) for each in texts[:-1]]
    pieces.append(piece(text(texts[-1]), finish=finish, counted=usage()))
    return streamed(*pieces)


def failed(status: int, message: str, kind: str) -> Any:
    return answer({"error": {"code": status, "message": message, "status": kind}}, status=status)


def model(name: str, *methods: str, **more: Any) -> dict[str, Any]:
    """An entry of the model listing."""
    entry: dict[str, Any] = {"name": f"models/{name}", "version": "001", **more}
    if methods:
        entry["supportedGenerationMethods"] = list(methods)
    return entry


HELLO_THERE = said("Hello there.")

# -- A request and its answer ---------------------------------------------------

case("plain_answer", "plain", GENERATE, GEMINI, HELLO, HELLO_THERE)
case("plain_stream", "plain stream", STREAM, GEMINI, HELLO, said("Hello", " there."))
case("plain_text", "plain stream", TEXT, GEMINI, HELLO, said("Hello", " there."))
case(
    "system_prompt",
    "system_prompt",
    GENERATE,
    GEMINI,
    asked(system_prompt="You answer in one sentence."),
    HELLO_THERE,
)
case(
    "several_turns",
    "turns",
    GENERATE,
    GEMINI,
    {
        "messages": [
            {"role": "user", "content": "Hello"},
            {"role": "assistant", "content": "Hello. How can I help?"},
            {"role": "user", "content": [{"type": "text", "text": "Where is my order?"}]},
        ]
    },
    said("Which order?"),
)
case(
    "temperature_and_token_bound_of_the_request",
    "sampling",
    GENERATE,
    GEMINI,
    asked(temperature=0.2, max_tokens=256),
    HELLO_THERE,
)
case(
    "token_bound_of_the_configuration",
    "sampling",
    GENERATE,
    {**GEMINI, "max_tokens": 512},
    HELLO,
    HELLO_THERE,
)
case(
    "a_model_of_the_configuration",
    "catalogue",
    GENERATE,
    {**GEMINI, "model": "gemini-2.5-flash"},
    HELLO,
    HELLO_THERE,
)
case(
    "a_model_the_catalogue_does_not_name",
    "catalogue",
    GENERATE,
    {**GEMINI, "model": "gemini-9-future"},
    HELLO,
    HELLO_THERE,
)
case(
    "a_history_ending_with_the_model",
    "turns",
    GENERATE,
    GEMINI,
    {
        "messages": [
            {"role": "user", "content": "Hello"},
            {"role": "assistant", "content": "Hello. How can I help?"},
        ]
    },
)

# -- Thinking --------------------------------------------------------------------

for _model in ("gemini-3.8-flash", "gemini-3.6-flash", "gemini-2.5-flash", "gemini-9-future"):
    for _effort in ("minimal", "low", "medium", "high", "xhigh", "none"):
        case(
            f"reasoning_effort_{_effort}_on_{_model}",
            "thinking",
            GENERATE,
            {**GEMINI, "model": _model},
            asked(reasoning_effort=_effort),
            HELLO_THERE,
        )
for _model in (
    "gemini-3.8-flash",
    "gemini-3.1-pro-preview",
    "gemini-3.5-flash-lite",
    "gemini-2.5-flash",
):
    case(
        f"thinking_turned_off_on_{_model}",
        "thinking",
        GENERATE,
        {**GEMINI, "model": _model},
        asked(enable_thinking=False),
        HELLO_THERE,
    )
    case(
        f"thinking_turned_on_on_{_model}",
        "thinking",
        GENERATE,
        {**GEMINI, "model": _model},
        asked(enable_thinking=True),
        HELLO_THERE,
    )
for _budget in (0, 1024, -1):
    case(
        f"thinking_budget_{_budget}_on_gemini-2.5-flash".replace("-1_", "dynamic_"),
        "thinking",
        GENERATE,
        {**GEMINI, "model": "gemini-2.5-flash"},
        asked(thinking_budget=_budget),
        HELLO_THERE,
    )
case(
    "thinking_level_of_the_configuration",
    "thinking",
    GENERATE,
    {**GEMINI, "thinking_level": "high"},
    HELLO,
    HELLO_THERE,
)
case(
    "thinking_level_of_the_configuration_under_the_request_s_effort",
    "thinking",
    GENERATE,
    {**GEMINI, "thinking_level": "high"},
    asked(reasoning_effort="low"),
    HELLO_THERE,
)
case(
    "thinking_level_of_the_configuration_on_a_model_without_levels",
    "thinking",
    GENERATE,
    {**GEMINI, "model": "gemini-2.5-flash", "thinking_level": "low"},
    HELLO,
    HELLO_THERE,
)
case(
    "thoughts_streamed",
    "thinking stream",
    STREAM,
    GEMINI,
    asked(reasoning_effort="high"),
    streamed(
        piece(thought("The user greets me.")),
        piece(thought(" I greet back.")),
        piece(text("Hello there."), finish="STOP", counted=usage(thoughtsTokenCount=9)),
    ),
)
case(
    "thoughts_read_whole",
    "thinking",
    GENERATE,
    GEMINI,
    asked(reasoning_effort="high"),
    streamed(
        piece(thought("The user greets me.")),
        piece(text("Hello there."), finish="STOP", counted=usage(thoughtsTokenCount=9)),
    ),
)

# -- Tools -----------------------------------------------------------------------

case("tools_declared", "tools", GENERATE, GEMINI, TOOLS, HELLO_THERE)
case(
    "a_call_answered",
    "tools calls",
    GENERATE,
    GEMINI,
    TOOLS,
    streamed(
        piece(call("lookup_order", {"number": "A-1"}, SIGNATURE), finish="STOP", counted=usage())
    ),
)
case(
    "a_call_streamed",
    "tools calls stream",
    STREAM,
    GEMINI,
    TOOLS,
    streamed(
        piece(text("Let me look.")),
        piece(call("lookup_order", {"number": "A-1"}, SIGNATURE), finish="STOP", counted=usage()),
    ),
)
case(
    "parallel_calls_the_first_signed",
    "tools calls",
    GENERATE,
    GEMINI,
    TOOLS,
    streamed(
        piece(
            call("lookup_order", {"number": "A-1"}, SIGNATURE),
            call("clock", {}),
            finish="STOP",
            counted=usage(),
        )
    ),
)
case(
    "a_call_emitted_again_without_its_signature",
    "tools calls",
    GENERATE,
    GEMINI,
    TOOLS,
    streamed(
        piece(call("lookup_order", {"number": "A-1"}, SIGNATURE)),
        piece(call("lookup_order", {"number": "A-1"}), finish="STOP", counted=usage()),
    ),
)
case(
    "two_identical_calls_of_one_chunk",
    "tools calls",
    GENERATE,
    GEMINI,
    TOOLS,
    streamed(
        piece(
            call("lookup_order", {"number": "A-1"}, SIGNATURE),
            call("lookup_order", {"number": "A-1"}),
            finish="STOP",
            counted=usage(),
        )
    ),
)
case(
    "a_dotted_tool_name",
    "tools",
    GENERATE,
    GEMINI,
    {**HELLO, "tools": [{**LOOKUP, "name": "orders.lookup"}]},
    HELLO_THERE,
)
case(
    "a_tool_name_gemini_refuses",
    "tools",
    GENERATE,
    GEMINI,
    {**HELLO, "tools": [{**LOOKUP, "name": "9_lookup"}]},
)


def round_of(signatures: tuple[str | None, str | None], *, earlier: bool = False) -> list[Any]:
    """A round of two calls and their results, each call signed as
    ``signatures`` says; ``earlier`` puts a user's message after it, so that
    the round belongs to an earlier turn."""
    calls = []
    for index, (name, args, signature) in enumerate(
        (("lookup_order", {"number": "A-1"}, signatures[0]), ("clock", {}, signatures[1])), 1
    ):
        part: dict[str, Any] = {
            "type": "tool_call",
            "id": f"call_{index}",
            "name": name,
            "arguments": args,
        }
        if signature is not None:
            part["metadata"] = {"thought_signature": signature}
        calls.append(part)
    messages = [
        {"role": "user", "content": "Where is order A-1?"},
        {"role": "assistant", "content": [{"type": "text", "text": "Let me look."}, *calls]},
        {
            "role": "tool",
            "content": [
                {
                    "type": "tool_result",
                    "tool_call_id": "call_1",
                    "name": "lookup_order",
                    "result": "Shipped.",
                },
                {
                    "type": "tool_result",
                    "tool_call_id": "call_2",
                    "name": "clock",
                    "result": "12:00",
                    "is_error": True,
                },
            ],
        },
    ]
    if earlier:
        messages += [
            {"role": "assistant", "content": "It shipped."},
            {"role": "user", "content": "Thanks. And order B-2?"},
        ]
    return messages


for _name, _signatures in (
    ("both_signed", (SIGNATURE, OTHER_SIGNATURE)),
    ("the_first_signed", (SIGNATURE, None)),
    ("the_second_signed", (None, SIGNATURE)),
    ("none_signed", (None, None)),
):
    case(
        f"a_round_replayed_{_name}",
        "tools round",
        GENERATE,
        GEMINI,
        {**TOOLS, "messages": round_of(_signatures)},
        HELLO_THERE,
    )
case(
    "a_round_none_signed_on_a_model_that_does_not_sign",
    "tools round",
    GENERATE,
    {**GEMINI, "model": "gemini-2.5-flash"},
    {**TOOLS, "messages": round_of((None, None))},
    HELLO_THERE,
)
case(
    "an_earlier_turn_s_round_none_signed",
    "tools round",
    GENERATE,
    GEMINI,
    {**TOOLS, "messages": round_of((None, None), earlier=True)},
    HELLO_THERE,
)

# -- Tool schemas, as Gemini declares them ---------------------------------------
# Each schema declared alone, as the parameters of one tool: what RoomKit's
# cleaning and the SDK make of it is what the request carries.

SCHEMAS: dict[str, Any] = {
    "nullable_by_any_of": {
        "type": "object",
        "properties": {"note": {"anyOf": [{"type": "string"}, {"type": "null"}]}},
    },
    "nullable_by_a_type_list": {
        "type": "object",
        "properties": {"note": {"type": ["string", "null"]}},
    },
    "a_wider_type_list": {
        "type": "object",
        "properties": {"value": {"type": ["string", "integer"]}},
    },
    "one_of_two_objects": {
        "type": "object",
        "properties": {
            "target": {
                "oneOf": [
                    {"type": "object", "properties": {"id": {"type": "string"}}},
                    {"type": "object", "properties": {"email": {"type": "string"}}},
                ]
            }
        },
    },
    "all_of_gathered": {
        "allOf": [
            {"type": "object", "properties": {"a": {"type": "string"}}, "required": ["a"]},
            {"properties": {"b": {"type": "integer"}}, "required": ["b"]},
        ]
    },
    "a_union_that_narrows_its_node": {
        "type": "object",
        "properties": {"id": {"type": "string"}, "email": {"type": "string"}},
        "anyOf": [{"required": ["id"]}, {"required": ["email"]}],
    },
    "untyped_with_properties": {"properties": {"q": {"type": "string"}}},
    "untyped_with_items": {
        "type": "object",
        "properties": {"tags": {"items": {"type": "string"}}},
    },
    "an_array_without_items": {"type": "object", "properties": {"tags": {"type": "array"}}},
    "a_string_enum": {
        "type": "object",
        "properties": {"color": {"type": "string", "enum": ["red", "green"]}},
    },
    "an_integer_enum": {
        "type": "object",
        "properties": {"level": {"type": "integer", "enum": [1, 2, 3]}},
    },
    "a_mixed_enum": {"type": "object", "properties": {"value": {"enum": ["a", 1, True]}}},
    "a_const": {"type": "object", "properties": {"kind": {"const": "order"}}},
    "keys_gemini_does_not_take": {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Probe",
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "q": {
                "type": "string",
                "default": "x",
                "examples": ["y"],
                "pattern": "^a",
                "minLength": 1,
            }
        },
    },
    "required_naming_a_missing_property": {
        "type": "object",
        "properties": {"a": {"type": "string"}},
        "required": ["a", "b"],
    },
    "required_naming_only_missing_properties": {
        "type": "object",
        "properties": {"a": {"type": "string"}},
        "required": ["b"],
    },
    "a_property_of_a_boolean_schema": {
        "type": "object",
        "properties": {"a": True, "b": {"type": "string"}},
        "required": ["a", "b"],
    },
    "nested_objects_in_an_array": {
        "type": "object",
        "properties": {
            "lines": {
                "type": "array",
                "description": "The order's lines",
                "items": {
                    "type": "object",
                    "properties": {
                        "sku": {"type": "string", "description": "The product"},
                        "quantity": {"type": "integer", "minimum": 1, "maximum": 99},
                    },
                    "required": ["sku"],
                },
                "minItems": 1,
                "maxItems": 10,
            }
        },
    },
    "formats_and_bounds": {
        "type": "object",
        "properties": {
            "at": {"type": "string", "format": "date-time"},
            "amount": {"type": "number", "minimum": 0, "maximum": 1000.5},
        },
    },
    "a_reference": {
        "type": "object",
        "$defs": {"Id": {"type": "string"}},
        "properties": {"id": {"$ref": "#/$defs/Id"}},
    },
    "an_explicit_nullable": {
        "type": "object",
        "properties": {"note": {"type": "string", "nullable": True}},
    },
    "an_object_without_properties": {"type": "object"},
    "a_union_inside_items": {
        "type": "object",
        "properties": {
            "values": {"type": "array", "items": {"anyOf": [{"type": "string"}, {"type": "null"}]}}
        },
    },
    "a_null_type_alone": {"type": "object", "properties": {"nothing": {"type": "null"}}},
    "booleans_and_numbers": {
        "type": "object",
        "properties": {"on": {"type": "boolean"}, "ratio": {"type": "number"}},
    },
    "a_type_the_sdk_refuses": {"type": "object", "properties": {"when": {"type": "date"}}},
}
for _name, _schema in SCHEMAS.items():
    case(
        f"tool_schema_{_name}",
        "tool_schema",
        GENERATE,
        GEMINI,
        {**HELLO, "tools": [{"name": "probe", "description": "A probe", "parameters": _schema}]},
        HELLO_THERE,
    )
case(
    "a_tool_without_parameters",
    "tool_schema",
    GENERATE,
    GEMINI,
    {**HELLO, "tools": [{"name": "clock", "description": "Tell the time"}]},
    HELLO_THERE,
)

# -- Images ----------------------------------------------------------------------

case(
    "an_image_inline",
    "images",
    GENERATE,
    GEMINI,
    {
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "What is this?"},
                    {"type": "image", "url": PNG},
                ],
            }
        ]
    },
    HELLO_THERE,
)
case(
    "an_image_by_its_url",
    "images",
    GENERATE,
    GEMINI,
    {
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "What is this?"},
                    {
                        "type": "image",
                        "url": "https://images.example/cat.png",
                        "mime_type": "image/png",
                    },
                ],
            }
        ]
    },
    HELLO_THERE,
)

# -- Response schema -------------------------------------------------------------

PARIS = '{"city": "Paris"}'
case(
    "a_response_schema",
    "response_schema",
    GENERATE,
    GEMINI,
    asked(response_schema=CITY),
    said(PARIS),
)
case(
    "a_response_schema_beside_tools",
    "response_schema tools",
    GENERATE,
    GEMINI,
    {**TOOLS, "response_schema": CITY},
    said(PARIS),
)
case(
    "a_response_schema_answered_otherwise",
    "response_schema",
    GENERATE,
    GEMINI,
    asked(response_schema=CITY),
    said("Paris."),
)

# -- How an answer ends ----------------------------------------------------------

for _finish in ("MAX_TOKENS", "SAFETY", "RECITATION", "MALFORMED_FUNCTION_CALL", "OTHER"):
    case(
        f"finished_{_finish.lower()}",
        "finish",
        GENERATE,
        GEMINI,
        HELLO,
        said("Hello", finish=_finish),
    )
case(
    "finished_max_tokens_with_no_part",
    "finish",
    GENERATE,
    GEMINI,
    HELLO,
    streamed(piece(text("Hel")), piece(finish="MAX_TOKENS", counted=usage())),
)
case(
    "a_prompt_blocked",
    "finish",
    GENERATE,
    GEMINI,
    HELLO,
    streamed(piece(block="SAFETY", counted=usage(candidates=0))),
)
case(
    "usage_with_thoughts_and_cache",
    "usage",
    GENERATE,
    GEMINI,
    HELLO,
    streamed(
        piece(
            text("Hello there."),
            finish="STOP",
            counted=usage(
                prompt=30, candidates=5, thoughtsTokenCount=11, cachedContentTokenCount=20
            ),
        )
    ),
)
case(
    "usage_told_on_each_chunk",
    "usage stream",
    STREAM,
    GEMINI,
    HELLO,
    streamed(
        piece(text("Hello"), counted=usage(candidates=1)),
        piece(text(" there."), finish="STOP", counted=usage(candidates=3)),
    ),
)
case("an_empty_stream", "stream", GENERATE, GEMINI, HELLO, stream([]))
case(
    "a_stream_without_candidates",
    "stream",
    GENERATE,
    GEMINI,
    HELLO,
    streamed(piece(counted=usage(candidates=0))),
)

# -- Failures --------------------------------------------------------------------

for _status, _kind in (
    (400, "INVALID_ARGUMENT"),
    (401, "UNAUTHENTICATED"),
    (403, "PERMISSION_DENIED"),
    (404, "NOT_FOUND"),
    (429, "RESOURCE_EXHAUSTED"),
    (500, "INTERNAL"),
    (503, "UNAVAILABLE"),
):
    case(
        f"status_{_status}",
        "failure",
        GENERATE,
        GEMINI,
        HELLO,
        failed(_status, f"The request failed ({_kind}).", _kind),
    )
case(
    "a_status_with_no_json",
    "failure",
    GENERATE,
    GEMINI,
    HELLO,
    page("<html>Bad gateway</html>", 502),
)
case(
    "a_stream_cut",
    "failure stream",
    STREAM,
    GEMINI,
    HELLO,
    streamed(piece(text("Hel")), cut=LOST),
)
case(
    "a_stream_timed_out",
    "failure stream",
    STREAM,
    GEMINI,
    HELLO,
    streamed(piece(text("Hel")), cut=TIMEOUT),
)

# -- The listing -----------------------------------------------------------------

case(
    "models_listed",
    "listing",
    LISTING,
    GEMINI,
    {},
    answer(
        {
            "models": [
                model(
                    "gemini-3.8-flash",
                    "generateContent",
                    "countTokens",
                    displayName="Gemini 3.8 Flash",
                    inputTokenLimit=1048576,
                ),
                model("gemini-10-new", "generateContent", inputTokenLimit=2000000),
                model("text-embedding-005", "embedContent"),
                model("gemini-embedding-002", "embedContent"),
                model("imagen-5", "predict"),
            ]
        }
    ),
)
case(
    "models_listed_on_two_pages",
    "listing",
    LISTING,
    GEMINI,
    {},
    answer({"models": [model("gemini-3.8-flash", "generateContent")], "nextPageToken": "page-2"}),
    answer({"models": [model("gemini-2.5-flash", "generateContent")]}),
)
case(
    "models_listed_without_their_actions",
    "listing",
    LISTING,
    GEMINI,
    {},
    answer(
        {
            "models": [
                model("gemini-2.5-flash", displayName="Gemini 2.5 Flash"),
                model("gemini-11-new"),
                model("gemini-embedding-001"),
                model("other-model"),
                {"version": "001"},
            ]
        }
    ),
)
case("models_listed_none", "listing", LISTING, GEMINI, {}, answer({}))
case(
    "models_listing_failed",
    "listing failure",
    LISTING,
    GEMINI,
    {},
    failed(401, "API key not valid.", "UNAUTHENTICATED"),
)

# Unused names a later table reads.
_ = Turn
