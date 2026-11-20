#!/usr/bin/env python3
"""Write what RoomKit's Python gives, for the contract's tests to compare with.

Two fixtures, each recording the RoomKit version it was written from:

- ``python_forms.json``: the JSON forms of the models. Each example holds the
  fields a caller gives a model (``given``) and what the model then dumps with
  ``model_dump(mode="json")`` (``form``). Each refusal holds an input Python
  refuses and the words of its ``ValueError``; each rejected input, one Python
  refuses in pydantic's own words, which the contract only has to refuse too.
  Each long float holds a number Python writes with 16 or 17 digits, its exact
  bits, and the JSON text of models that carry it. The Rust test
  ``python_forms`` checks that ``given`` reads as ``form``, that ``form`` reads
  and writes back unchanged, that a refusal is refused in the same words, that
  a rejected input is refused, and that a long float is read to the bit.
- ``python_call_rules.json``: what the rules on a call's arguments decide for
  each raw value, ending and position, and the mapping an argument text reads
  as. The Rust test ``python_call_rules`` checks that its rules decide the
  same.

Run it with a Python interpreter that has ``roomkit`` installed
(``make python-forms``).
"""

from __future__ import annotations

import json
import struct
from datetime import date
from pathlib import Path
from typing import Any

import roomkit
from roomkit.providers.ai import (
    AIContext,
    AIImagePart,
    AIMessage,
    AIResponse,
    AITextPart,
    AIThinkingPart,
    AITool,
    AIToolCall,
    AIToolCallPart,
    AIToolResultPart,
    ModelInfo,
    ModelPricing,
    ServedCall,
    StreamDone,
    StreamTextDelta,
    StreamThinkingDelta,
    StreamToolCall,
    StreamToolCallDelta,
    StreamUsage,
    call_cut,
    call_garbled,
    call_partial,
    tool_arguments,
    unreadable_arguments,
)

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURES = REPOSITORY / "crates/roomkit-providers-core/tests/fixtures"
FORMS = FIXTURES / "python_forms.json"
CALL_RULES = FIXTURES / "python_call_rules.json"

# The fields of ``AIContext`` a provider reads: the contract's request.
REQUEST_FIELDS = (
    "messages",
    "system_prompt",
    "temperature",
    "max_tokens",
    "thinking_budget",
    "enable_thinking",
    "reasoning_effort",
    "tools",
    "response_schema",
    "metadata",
)

# The fields of ``AIResponse`` a host attaches, which no provider fills: the
# contract's response leaves them out.
HOST_RESPONSE_FIELDS = ("tasks", "observations")

OUTCOMES = ("served", "refused", "failed", "blocked", "unserved", "cancelled")

PNG = "data:image/png;base64,iVBORw0KGgo="

TEXT = {"type": "text", "text": "Here is the tracking page."}
IMAGE = {"type": "image", "url": PNG, "mime_type": "image/png"}
REMOTE_IMAGE = {"type": "image", "url": "https://example.com/label.png"}
THINKING = {
    "type": "thinking",
    "thinking": "The user asks where the order is; look it up first.",
    "signature": "c2lnbmF0dXJl",
}
TOOL_CALL = {
    "type": "tool_call",
    "id": "call_1",
    "name": "lookup_order",
    "arguments": {"order": "A-1001"},
}
TEXT_RESULT = {
    "type": "tool_result",
    "tool_call_id": "call_1",
    "name": "lookup_order",
    "result": '{"status": "shipped"}',
    "outcome": "served",
}
PARTS_RESULT = {
    "type": "tool_result",
    "tool_call_id": "call_2",
    "name": "open_tracking_page",
    "result": [TEXT, IMAGE],
}

ORDER_PARAMETERS = {
    "type": "object",
    "properties": {
        "order": {"type": "string", "description": "The order number"},
        "include": {"type": "array", "items": {"type": "string"}},
    },
    "required": ["order"],
}
LOOKUP_TOOL = {"name": "lookup_order", "description": "Find an order by its number"}
ANSWER_SCHEMA = {
    "type": "object",
    "properties": {
        "status": {"type": "string"},
        "items": {"type": "array", "items": {"type": "string"}},
    },
    "required": ["status", "items"],
    "additionalProperties": False,
}

CONVERSATION = [
    {"role": "system", "content": "You answer in one sentence."},
    {"role": "user", "content": [{"type": "text", "text": "Where is this order?"}, IMAGE]},
    {
        "role": "assistant",
        "content": [THINKING, {"type": "text", "text": "Let me check."}, TOOL_CALL],
        "metadata": {"response_id": "resp_1"},
    },
    {"role": "tool", "content": [TEXT_RESULT]},
    {"role": "assistant", "content": "It shipped on Monday."},
]


CALL = {"id": "call_1", "name": "lookup_order"}
CALL_WITH_EVERYTHING = {
    "id": "call_2",
    "name": "lookup_order",
    "arguments": {"order": "A-1001", "include": ["items"]},
    "metadata": {"index": 1, "vendor": {"item_id": "fc_2"}},
    "served": {"result": "Order not found", "is_error": True},
    "partial": True,
    "garbled": True,
    "intent": "Look the order up",
}
USAGE = {"input_tokens": 1200, "output_tokens": 45, "cache_read_input_tokens": 800}
PRICING = {"input_per_million": 1.25, "output_per_million": 10, "verified": "2026-08-05"}
PRICING_WITH_EVERY_RATE = {
    "input_per_million": 2.5,
    "output_per_million": 15.0,
    "cache_read_per_million": 0.25,
    "cache_write_per_million": 3.125,
    "image_input_per_million": 5.0,
    "image_output_per_million": 30.0,
    "long_context_threshold_tokens": 200000,
    "long_context_input_multiplier": 2.0,
    "long_context_output_multiplier": 1.5,
    "currency": "EUR",
    "verified": "2024-02-29",
}


def result(**fields: Any) -> dict[str, Any]:
    """The fields of a tool result of the call ``call_1``."""
    return {"tool_call_id": "call_1", "name": "lookup_order", **fields}


EXAMPLES: list[tuple[type, str, dict[str, Any]]] = [
    (AITextPart, "text", {"text": "The order shipped on Monday."}),
    (AITextPart, "text_empty_with_its_type", {"type": "text", "text": ""}),
    (AIImagePart, "image_url_alone", {"url": "https://example.com/chart.png"}),
    (AIImagePart, "image_data_url_with_its_media_type", {"url": PNG, "mime_type": "image/png"}),
    (AIThinkingPart, "thinking", {"thinking": "Look the order up first."}),
    (
        AIThinkingPart,
        "thinking_signed",
        {"thinking": "Look the order up first.", "signature": "c2lnbmF0dXJl"},
    ),
    (AIThinkingPart, "thinking_redacted", {"thinking": "", "redacted": "b3BhcXVlIGRhdGE="}),
    (AIToolCallPart, "tool_call", {"id": "call_1", "name": "lookup_order"}),
    (
        AIToolCallPart,
        "tool_call_with_arguments",
        {
            "id": "call_2",
            "name": "lookup_order",
            "arguments": {
                "order": "A-1001",
                "include": ["items", "tracking"],
                "limit": 3,
                "ratio": 0.5,
                "verbose": False,
                "note": None,
                "filter": {"status": "open"},
            },
            "metadata": {"index": 0, "vendor": {"item_id": "fc_1"}},
        },
    ),
    (AIToolResultPart, "tool_result_text", result(result='{"status": "shipped"}')),
    (AIToolResultPart, "tool_result_text_and_image", result(result=[TEXT, IMAGE])),
    (AIToolResultPart, "tool_result_text_parts_alone", result(result=[TEXT, TEXT])),
    (AIToolResultPart, "tool_result_image_alone", result(result=[REMOTE_IMAGE])),
    (AIToolResultPart, "tool_result_no_part", result(result=[])),
    (
        AIToolResultPart,
        "tool_result_error",
        result(result='{"error": "Order not found"}', is_error=True, outcome="failed"),
    ),
    (
        AIToolResultPart,
        "tool_result_with_structured_content_and_references",
        result(
            result="2 items",
            structured_content={"order": {"id": "A-1001", "items": 2}},
            references=["cancel_order", "refund_order"],
        ),
    ),
    *(
        (AIToolResultPart, f"tool_result_{outcome}", result(result="done", outcome=outcome))
        for outcome in OUTCOMES
    ),
    (AITool, "tool", LOOKUP_TOOL),
    (
        AITool,
        "tool_with_parameters_tags_and_flags",
        {
            "name": "orders.lookup:v2-beta_1",
            "description": "Find an order by its number",
            "parameters": ORDER_PARAMETERS,
            "tags": ["order", "tracking"],
            "defer_loading": True,
            "changes_state": True,
        },
    ),
    (AIMessage, "message_system_text", {"role": "system", "content": "Answer in one sentence."}),
    (AIMessage, "message_user_text", {"role": "user", "content": "Where is my order A-1001?"}),
    (AIMessage, "message_user_text_and_image", {"role": "user", "content": [TEXT, REMOTE_IMAGE]}),
    (
        AIMessage,
        "message_assistant_thinking_text_and_call",
        {
            "role": "assistant",
            "content": [THINKING, {"type": "text", "text": "Let me check."}, TOOL_CALL],
            "metadata": {"response_id": "resp_1"},
        },
    ),
    (AIMessage, "message_tool_results", {"role": "tool", "content": [TEXT_RESULT, PARTS_RESULT]}),
    (AIMessage, "message_assistant_empty_text", {"role": "assistant", "content": ""}),
    (AIMessage, "message_user_no_part", {"role": "user", "content": []}),
    (AIContext, "request_defaults", {}),
    (
        AIContext,
        "request",
        {
            "messages": CONVERSATION,
            "system_prompt": "You help customers track their orders.",
            "temperature": 0.2,
            "max_tokens": 1024,
            "thinking_budget": 2048,
            "enable_thinking": True,
            "reasoning_effort": "high",
            "tools": [LOOKUP_TOOL, {**LOOKUP_TOOL, "name": "cancel_order", "changes_state": True}],
            "response_schema": ANSWER_SCHEMA,
            "metadata": {"tenant": "acme", "trace": {"id": "trace-1", "sampled": True}},
        },
    ),
    (
        AIContext,
        "request_reasoning_off_whole_temperature",
        {
            "messages": [{"role": "user", "content": "Hello"}],
            "temperature": 1,
            "thinking_budget": 0,
            "enable_thinking": False,
            "reasoning_effort": "none",
        },
    ),
    # The credential of one request, a non-empty string under the `api_key`
    # of the request's own metadata, is dumped masked; the same key anywhere
    # else is data like any other.
    (
        AIContext,
        "request_credential_masked",
        {
            "messages": [
                {"role": "user", "content": "Hello", "metadata": {"api_key": "not-a-key-either"}}
            ],
            "metadata": {
                "tenant": "acme",
                "api_key": "placeholder-not-a-key",
                "upstream": {"api_key": "nested-not-a-key"},
            },
        },
    ),
    (AIContext, "request_credential_empty", {"metadata": {"api_key": ""}}),
    (
        AIContext,
        "request_credential_not_a_string",
        {"metadata": {"api_key": 42, "tenant": "acme"}},
    ),
    (
        AIContext,
        "request_long_floats",
        {"temperature": 20708.635217891522, "metadata": {"ratio": -948.7292200260417}},
    ),
    (ServedCall, "served_call_defaults", {}),
    (ServedCall, "served_call_failed", {"result": "Order not found", "is_error": True}),
    (AIToolCall, "call", CALL),
    (AIToolCall, "call_served_by_default", {**CALL, "served": {}}),
    (AIToolCall, "call_with_everything", CALL_WITH_EVERYTHING),
    (
        AIToolCall,
        "call_long_float_arguments",
        {**CALL, "arguments": {"ratio": 20708.635217891522, "delta": -948.7292200260417}},
    ),
    (AIResponse, "response_text", {"content": "It shipped on Monday."}),
    (
        AIResponse,
        "response_with_calls",
        {
            "content": "",
            "finish_reason": "tool_calls",
            "usage": USAGE,
            "metadata": {"model": "example-large-1"},
            "tool_calls": [CALL, CALL_WITH_EVERYTHING],
        },
    ),
    (
        AIResponse,
        "response_with_thinking_blocks",
        {
            "content": "It shipped on Monday.",
            "thinking": "Look the order up first.",
            "thinking_signature": "c2lnbmF0dXJl",
            "thinking_parts": [
                {"thinking": "Look the order up first.", "signature": "c2lnbmF0dXJl"},
                {"type": "thinking", "thinking": "", "redacted": "b3BhcXVlIGRhdGE="},
            ],
            "finish_reason": "stop",
            "usage": {"output_tokens": 30, "reasoning_tokens": 12},
            "metadata": {"model": "example-large-1", "refusal": "I cannot help with that."},
        },
    ),
    (AIResponse, "response_no_thinking_block", {"content": "", "thinking_parts": []}),
    (StreamTextDelta, "text_delta", {"text": "It shipped"}),
    (StreamThinkingDelta, "thinking_delta", {"thinking": "Look the order up first."}),
    (
        StreamThinkingDelta,
        "thinking_delta_signature_alone",
        {"thinking": "", "signature": "c2lnbmF0dXJl", "block": 0},
    ),
    (
        StreamThinkingDelta,
        "thinking_delta_redacted",
        {"type": "thinking_delta", "thinking": "", "block": 1, "redacted": "b3BhcXVlIGRhdGE="},
    ),
    (StreamToolCallDelta, "tool_call_delta_opening", CALL),
    (
        StreamToolCallDelta,
        "tool_call_delta_fragment",
        {**CALL, "index": 2, "arguments_delta": '{"order": "A-'},
    ),
    (StreamToolCall, "stream_tool_call", CALL),
    (StreamToolCall, "stream_tool_call_with_everything", CALL_WITH_EVERYTHING),
    (StreamUsage, "stream_usage_defaults", {}),
    (StreamUsage, "stream_usage", {"usage": {"input_tokens": 1200}}),
    (StreamDone, "stream_done_defaults", {}),
    (
        StreamDone,
        "stream_done",
        {"finish_reason": "stop", "usage": USAGE, "metadata": {"model": "example-large-1"}},
    ),
    (ModelPricing, "pricing", PRICING),
    (ModelPricing, "pricing_with_every_rate", PRICING_WITH_EVERY_RATE),
    (ModelInfo, "model", {"id": "example-small-1"}),
    (
        ModelInfo,
        "model_with_everything",
        {
            "id": "example-large-1",
            "display_name": "Example Large 1",
            "context_window": 400000,
            "supports_vision": True,
            "deprecated": True,
            "capabilities": ["tools", "vision"],
            "pricing": PRICING_WITH_EVERY_RATE,
        },
    ),
    (
        ModelInfo,
        "model_without_vision_at_a_flat_price",
        {"id": "example-text-1", "supports_vision": False, "pricing": PRICING},
    ),
]


PORTABLE_SCHEMA = {
    "type": "object",
    "properties": {"label": {"type": "string"}, "count": {"type": "integer"}},
    "required": ["label", "count"],
    "additionalProperties": False,
}

# Texts Python's ``repr`` quotes its own way: one holds a quote, one a line
# break, one a no-break space. A refusal that names what it refuses quotes it
# so.
QUOTED = ("it's", "a\n", "x" + chr(0xA0) + "y")

# What a model refuses when it is built: a response schema outside the
# portable subset, a tool name no vendor accepts.
REFUSALS: list[tuple[type, str, dict[str, Any]]] = [
    (
        AIContext,
        "request_schema_root_not_an_object",
        {"response_schema": {"type": "array", "items": {"type": "string"}}},
    ),
    (
        AIContext,
        "request_schema_object_left_open",
        {"response_schema": {**PORTABLE_SCHEMA, "additionalProperties": True}},
    ),
    (
        AIContext,
        "request_schema_keyword_not_portable",
        {
            "response_schema": {
                **PORTABLE_SCHEMA,
                "properties": {
                    "label": {"type": "string"},
                    "count": {"type": "integer", "minimum": 0},
                },
            }
        },
    ),
    (
        AIContext,
        "request_schema_property_not_required",
        {"response_schema": {**PORTABLE_SCHEMA, "required": ["label"]}},
    ),
    (AITool, "tool_name_no_vendor_accepts", {"name": "not allowed", "description": "Unused"}),
    *(
        (AITool, f"tool_name_quoted_{at}", {"name": text, "description": "Unused"})
        for at, text in enumerate(QUOTED)
    ),
]

# Texts that write no day, or a day that does not exist.
REFUSED_DAYS = (*QUOTED, "2026-8-05", "2026-13-01", "2026-02-30", "0000-01-01")

# The union of the events of a stream: an input is one of them when one of its
# models takes it.
STREAM_EVENTS = (
    StreamTextDelta,
    StreamThinkingDelta,
    StreamToolCall,
    StreamToolCallDelta,
    StreamUsage,
    StreamDone,
)
MODELS: dict[str, tuple[type, ...]] = {
    model.__name__: (model,)
    for model in (
        AIContext,
        AIImagePart,
        AIMessage,
        AIResponse,
        AITextPart,
        AIThinkingPart,
        AITool,
        AIToolCall,
        AIToolCallPart,
        AIToolResultPart,
        ModelInfo,
        ModelPricing,
        ServedCall,
        *STREAM_EVENTS,
    )
} | {"StreamEvent": STREAM_EVENTS}

TOOL_RESULT = {"tool_call_id": "call_1", "name": "lookup_order", "result": "done"}

# Inputs Python refuses in pydantic's own words: a model given as a list of
# its values, a name or a `type` given as anything but a text.
REJECTED: list[tuple[str, str, Any]] = [
    ("AIContext", "request_as_its_values", [[], "Answer briefly.", 0.1]),
    ("AIToolCall", "call_as_its_values", ["call_1", "lookup_order"]),
    ("AIMessage", "message_as_its_values", ["user", "hi"]),
    ("StreamEvent", "event_as_its_type", ["done"]),
    ("StreamEvent", "event_type_a_number", {"type": 2}),
    (
        "ModelPricing",
        "pricing_as_its_values",
        [1.0, 2.0, None, None, None, None, None, 1.0, 1.0, "USD", "2026-08-05"],
    ),
    ("AITextPart", "part_type_a_mapping", {"type": {"text": None}, "text": "hi"}),
    ("AITextPart", "part_type_a_number", {"type": 0, "text": "hi"}),
    ("AIMessage", "message_role_a_mapping", {"role": {"user": None}, "content": "hi"}),
    ("AIMessage", "message_role_a_number", {"role": 1, "content": "hi"}),
    (
        "AIToolResultPart",
        "tool_result_outcome_a_mapping",
        {**TOOL_RESULT, "outcome": {"served": None}},
    ),
    (
        "AIMessage",
        "message_part_type_a_number",
        {"role": "user", "content": [{"type": 0, "text": "hi"}]},
    ),
    (
        "AIMessage",
        "message_tool_result_part_type_a_number",
        {
            "role": "tool",
            "content": [
                {**TOOL_RESULT, "type": "tool_result", "result": [{"type": 0, "text": "x"}]}
            ],
        },
    ),
    *((label, f"{label}_from_a_list", []) for label in MODELS),
    *((label, f"{label}_from_null", None) for label in MODELS),
]

# Floats Python writes with 16 or 17 significant digits, which a parser reads
# as a neighbouring float unless it rounds correctly. None is written with an
# exponent, so the text Python writes is the text the contract writes.
LONG_FLOATS = (20708.635217891522, -948.7292200260417, 0.30000000000000004, 123456789.12345679)

# The raw arguments a wire can carry for a call: nothing, a mapping, a text
# that reads, one that does not, another JSON value.
RAW_ARGUMENTS: list[Any] = [
    None,
    {},
    {"city": "Lisbon"},
    " ",
    "null",
    '{"k": 1}',
    "[]",
    '{"x":',
    9,
    True,
    ["one"],
]
ENDINGS: list[tuple[str | None, bool]] = [
    ("stop", True),
    ("MAX_TOKENS", True),
    ("content_filter", False),
    (None, True),
]

# Texts Python's reading of whitespace and of numbers decides: a no-break
# space is no blank, a unit separator is one, and a number JSON does not name
# makes the arguments unreadable.
EDGE_TEXTS: list[str] = [
    chr(0xA0) + "{}" + chr(0xA0),
    chr(0x1F),
    '{"record":[{"amount":NaN}]}',
    '{"record":[{"amount":Infinity}]}',
    '{"record":[{"amount":-Infinity}]}',
    '{"record":[{"amount":1e900}]}',
    '{"record":[{"amount":-1e900}]}',
    '{"record":[{"amount":1e300}]}',
    '{"record":[{"amount":"NaN"}]}',
]
EDGE_ENDINGS: list[tuple[str | None, bool]] = [("stop", True), ("length", True)]

# Argument texts beside the ones above: numbers of 16 and 17 digits.
ARGUMENT_TEXTS: list[str] = ['{"ratio": 20708.635217891522, "delta": -948.7292200260417}']


# The models whose dump is restricted to what the contract carries.
DUMPED = {
    AIContext: {"include": set(REQUEST_FIELDS)},
    AIResponse: {"exclude": set(HOST_RESPONSE_FIELDS)},
}


def form(model: type, given: dict[str, Any]) -> Any:
    """What ``model`` dumps once built from ``given``."""
    return model.model_validate(given).model_dump(mode="json", **DUMPED.get(model, {}))


def refusal(model: type, given: dict[str, Any]) -> str:
    """The words of the ``ValueError`` ``model`` refuses ``given`` with."""
    try:
        model.model_validate(given)
    except ValueError as refused:
        (error,) = refused.errors()
        return str(error["ctx"]["error"])
    raise AssertionError(f"{model.__name__} takes {given!r}")


def refuses(model: type, given: Any) -> bool:
    """Whether ``model`` refuses ``given``."""
    try:
        model.model_validate(given)
    except ValueError:
        return True
    return False


def refused_day(text: str) -> str:
    """The words ``date.fromisoformat`` refuses ``text`` with."""
    try:
        date.fromisoformat(text)
    except ValueError as refused:
        return str(refused)
    raise AssertionError(f"{text!r} writes a day")


def refused_counter(name: str) -> dict[str, Any]:
    """A response whose usage counter ``name`` is no integer, which Python
    refuses in pydantic's words. The contract's words name the counter, quoted
    as Python quotes a text."""
    given = {"content": "", "usage": {name: 1.5}}
    if not refuses(AIResponse, given):
        raise AssertionError(f"AIResponse takes {given!r}")
    return {"given": given, "error": f"usage counter {name!r} must be an integer"}


def rejected(label: str, name: str, given: Any) -> dict[str, Any]:
    """An input every model of ``label`` refuses."""
    if not all(refuses(model, given) for model in MODELS[label]):
        raise AssertionError(f"{label} takes {given!r}")
    return {"model": label, "name": name, "given": given}


def compact(value: Any) -> str:
    """``value`` as JSON text without spaces, as the contract writes it."""
    return json.dumps(value, separators=(",", ":"))


def long_float(value: float) -> dict[str, Any]:
    """A float with its exact bits, and the JSON text of models carrying it."""
    request = {"temperature": value, "metadata": {"ratio": value}}
    call = {"id": "call_1", "name": "lookup_order", "arguments": {"ratio": value}}
    return {
        "text": repr(value),
        "bits": int.from_bytes(struct.pack(">d", value), "big"),
        "request": compact(form(AIContext, request)),
        "call": compact(form(AIToolCall, call)),
        "arguments": json.dumps({"ratio": value}),
    }


def call_rule(raw: Any, finish_reason: str | None, last: bool) -> dict[str, Any]:
    """What the rules decide for the arguments ``raw`` of a call."""
    return {
        "raw": raw,
        "finish_reason": finish_reason,
        "last": last,
        "unreadable": unreadable_arguments(raw),
        "partial": call_partial(raw, finish_reason, last=last),
        "cut": call_cut(raw, finish_reason, last=last),
        "garbled": call_garbled(raw, finish_reason, last=last),
    }


def forms() -> dict[str, Any]:
    return {
        "generator": "scripts/generate_python_forms.py",
        "roomkit_version": roomkit.__version__,
        "request_fields": list(REQUEST_FIELDS),
        "response_fields_left_out": list(HOST_RESPONSE_FIELDS),
        "examples": [
            {"model": model.__name__, "name": name, "given": given, "form": form(model, given)}
            for model, name, given in EXAMPLES
        ],
        "refusals": [
            *(
                {
                    "model": model.__name__,
                    "name": name,
                    "given": given,
                    "error": refusal(model, given),
                }
                for model, name, given in REFUSALS
            ),
            *(
                {"model": "date", "name": f"day_{at}", "given": text, "error": refused_day(text)}
                for at, text in enumerate(REFUSED_DAYS)
            ),
            *(
                {
                    "model": "AIResponse",
                    "name": f"usage_counter_quoted_{at}",
                    **refused_counter(text),
                }
                for at, text in enumerate(QUOTED)
            ),
        ],
        "rejected": [rejected(label, name, given) for label, name, given in REJECTED],
        "long_floats": [long_float(value) for value in LONG_FLOATS],
    }


def call_rules() -> dict[str, Any]:
    cases = [(raw, ending) for raw in RAW_ARGUMENTS for ending in ENDINGS]
    cases += [(text, ending) for text in EDGE_TEXTS for ending in EDGE_ENDINGS]
    texts = [raw for raw in [*RAW_ARGUMENTS, *EDGE_TEXTS] if isinstance(raw, str)]
    texts += ARGUMENT_TEXTS
    return {
        "generator": "scripts/generate_python_forms.py",
        "roomkit_version": roomkit.__version__,
        "cases": [call_rule(raw, finish_reason, last) for raw, (finish_reason, last) in cases],
        "arguments": [{"raw": text, "arguments": tool_arguments(text)} for text in texts],
    }


def write(path: Path, fixture: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    # Escaped, so that a no-break space or a control character shows.
    text = json.dumps(fixture, indent=4)
    path.write_text(text + "\n", encoding="utf-8")
    print(f"Wrote {path.relative_to(REPOSITORY)} from roomkit {roomkit.__version__}")


def main() -> None:
    write(FORMS, forms())
    write(CALL_RULES, call_rules())


if __name__ == "__main__":
    main()
