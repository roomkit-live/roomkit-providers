"""What Anthropic's Messages API answers, written for a local fake of its
endpoint (``fake_endpoint``): the events of a streamed message, an error
body, a page of models.

Every generation is asked as a stream, a whole answer too: a message is the
reads of its events, one event a read. A block is what opens it and its
deltas; ``message`` numbers the blocks it is given and writes each one's
start, its deltas and its stop between the message's start and its end.
"""

from __future__ import annotations

import json
from typing import Any

from fake_endpoint import DEADLINE, EVENT_STREAM, JSON, Reads, Whole

# A content block of a streamed message: what its start holds, and its deltas.
Block = tuple[dict[str, Any], list[dict[str, Any]]]

MODEL = "local-model"
# What signs a block of reasoning, and the opaque data of a redacted one:
# base64 of placeholders, as the API writes both.
SIGNATURE = "c2lnbmF0dXJlLW9uZQ=="
OTHER_SIGNATURE = "c2lnbmF0dXJlLXR3bw=="
REDACTED = "cmVkYWN0ZWQtZGF0YQ=="


def event(data: dict[str, Any]) -> bytes:
    """One event of a stream, named by its type."""
    return f"event: {data['type']}\ndata: {json.dumps(data)}\n\n".encode()


def text(*pieces: str) -> Block:
    """A block of text, said piece by piece."""
    return {"type": "text", "text": ""}, [{"type": "text_delta", "text": piece} for piece in pieces]


def thinking(*pieces: str, signature: str = SIGNATURE) -> Block:
    """A block of reasoning, thought piece by piece, then its signature.
    With no piece it is a signature alone."""
    deltas = [{"type": "thinking_delta", "thinking": piece} for piece in pieces]
    deltas.append({"type": "signature_delta", "signature": signature})
    return {"type": "thinking", "thinking": "", "signature": ""}, deltas


def redacted(data: str = REDACTED) -> Block:
    """A block of reasoning the vendor redacted: its opaque data, whole at
    its start."""
    return {"type": "redacted_thinking", "data": data}, []


def call(name: str, arguments: str, call_id: str = "toolu_1") -> Block:
    """A call of the tool `name` with `arguments`, its JSON text."""
    opens = {"type": "tool_use", "id": call_id, "name": name, "input": {}}
    return opens, [{"type": "input_json_delta", "partial_json": arguments}]


def message_start(model: str = MODEL) -> bytes:
    """The event a message starts with: what it tells of its input."""
    usage = {
        "input_tokens": 12,
        "cache_creation_input_tokens": 0,
        "cache_read_input_tokens": 0,
        "output_tokens": 1,
    }
    started = {
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [],
        "stop_reason": None,
        "stop_sequence": None,
        "usage": usage,
    }
    return event({"type": "message_start", "message": started})


def block_events(index: int, block: Block) -> list[bytes]:
    """The events of one block: its start, its deltas, its stop."""
    opens, deltas = block
    return [
        event({"type": "content_block_start", "index": index, "content_block": opens}),
        *(
            event({"type": "content_block_delta", "index": index, "delta": delta})
            for delta in deltas
        ),
        event({"type": "content_block_stop", "index": index}),
    ]


def message(
    *blocks: Block,
    stop: str = "end_turn",
    model: str = MODEL,
    details: dict[str, Any] | None = None,
) -> list[bytes]:
    """The reads of a streamed message that holds `blocks` and stops for
    `stop`, one event a read. `details` is what a refusal says of itself."""
    reads = [message_start(model)]
    for index, block in enumerate(blocks):
        reads += block_events(index, block)
    ended: dict[str, Any] = {"stop_reason": stop, "stop_sequence": None}
    if details is not None:
        ended["stop_details"] = details
    reads.append(event({"type": "message_delta", "delta": ended, "usage": {"output_tokens": 5}}))
    return [*reads, event({"type": "message_stop"})]


def answers(*blocks: Block, **how: Any) -> Reads:
    """The reply of a message that holds `blocks`."""
    return Reads(200, EVENT_STREAM, message(*blocks, **how))


def says(*pieces: str, **how: Any) -> Reads:
    """The reply of a message that says `pieces`."""
    return answers(text(*pieces), **how)


def calls(name: str, arguments: str, *before: Block) -> Reads:
    """The reply of a message that calls the tool `name`, after `before`."""
    return answers(*before, call(name, arguments), stop="tool_use")


def declines(category: str = "cyber") -> Reads:
    """The reply of a message the model declined to write."""
    details = {"type": "refusal", "category": category, "explanation": "The request was declined."}
    return answers(stop="refusal", details=details)


def opens_and_waits(*pieces: str) -> Reads:
    """The reply of a message that starts, says `pieces`, then keeps its
    connection open and sends nothing more."""
    opens, deltas = text(*pieces)
    reads = [
        message_start(),
        event({"type": "content_block_start", "index": 0, "content_block": opens}),
        *(event({"type": "content_block_delta", "index": 0, "delta": delta}) for delta in deltas),
    ]
    return Reads(200, EVENT_STREAM, reads, then="silent")


def error(kind: str, said: str) -> bytes:
    """The body of a request the endpoint refuses."""
    body = {"type": "error", "error": {"type": kind, "message": said}, "request_id": "req_1"}
    return json.dumps(body).encode()


def refuses(status: int, kind: str, said: str) -> Whole:
    """The reply of a request the endpoint refuses with `status`."""
    return Whole(status, JSON, error(kind, said))


def models(*listed: tuple[str, str | None]) -> bytes:
    """The one page of a model listing: each model's id, and its name where
    the endpoint gives one."""
    data = []
    for model, name in listed:
        entry = {"type": "model", "id": model, "created_at": "2026-01-01T00:00:00Z"}
        data.append(entry if name is None else {**entry, "display_name": name})
    page = {
        "data": data,
        "has_more": False,
        "first_id": listed[0][0] if listed else None,
        "last_id": listed[-1][0] if listed else None,
    }
    return json.dumps(page).encode()


def local_config(port: int) -> dict[str, object]:
    """The members of the configuration of a provider that asks the fake on
    `port`: the base URL is the server's, to which the provider adds the
    path of the API. Its timeout is far past a test's deadline: a provider's
    own timeout never ends a wait in the place of what a test waits for."""
    return {
        "api_key": "placeholder-not-a-key",
        "model": MODEL,
        "base_url": f"http://127.0.0.1:{port}",
        "timeout": 100 * DEADLINE,
    }
