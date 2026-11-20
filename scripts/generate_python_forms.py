#!/usr/bin/env python3
"""Write the JSON forms of RoomKit's Python models, for the contract's tests.

Each example holds the fields a caller gives a model (``given``) and what the
model then dumps with ``model_dump(mode="json")`` (``form``). The Rust test
``python_forms`` checks, for each one, that ``given`` reads as ``form`` and
that ``form`` reads and writes back unchanged.

Run it with a Python interpreter that has ``roomkit`` installed
(``make python-forms``). The fixture records the RoomKit version it was
written from.
"""

from __future__ import annotations

import json
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
)

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURE = REPOSITORY / "crates/roomkit-providers-core/tests/fixtures/python_forms.json"

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
    (ServedCall, "served_call_defaults", {}),
    (ServedCall, "served_call_failed", {"result": "Order not found", "is_error": True}),
    (AIToolCall, "call", CALL),
    (AIToolCall, "call_served_by_default", {**CALL, "served": {}}),
    (AIToolCall, "call_with_everything", CALL_WITH_EVERYTHING),
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


# The models whose dump is restricted to what the contract carries.
DUMPED = {
    AIContext: {"include": set(REQUEST_FIELDS)},
    AIResponse: {"exclude": set(HOST_RESPONSE_FIELDS)},
}


def form(model: type, given: dict[str, Any]) -> Any:
    """What ``model`` dumps once built from ``given``."""
    return model.model_validate(given).model_dump(mode="json", **DUMPED.get(model, {}))


def main() -> None:
    fixture = {
        "generator": "scripts/generate_python_forms.py",
        "roomkit_version": roomkit.__version__,
        "request_fields": list(REQUEST_FIELDS),
        "response_fields_left_out": list(HOST_RESPONSE_FIELDS),
        "examples": [
            {"model": model.__name__, "name": name, "given": given, "form": form(model, given)}
            for model, name, given in EXAMPLES
        ],
    }
    FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps(fixture, indent=4, ensure_ascii=False)
    FIXTURE.write_text(text + "\n", encoding="utf-8")
    print(f"Wrote {FIXTURE.relative_to(REPOSITORY)} from roomkit {roomkit.__version__}")


if __name__ == "__main__":
    main()
