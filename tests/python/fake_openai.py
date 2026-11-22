"""What the fake answers as OpenAI's endpoint: the bodies of Chat
Completions, whole and streamed, and the configuration of a provider that
asks the fake (``fake_endpoint``).
"""

from __future__ import annotations

import json

from fake_endpoint import DEADLINE


def completion(content: str, model: str = "local-model") -> bytes:
    """The body of a whole answer that says `content`."""
    message = {"role": "assistant", "content": content}
    body = {
        "id": "chatcmpl-1",
        "object": "chat.completion",
        "model": model,
        "choices": [{"index": 0, "message": message, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 12, "completion_tokens": 5, "total_tokens": 17},
    }
    return json.dumps(body).encode()


def events(*pieces: str, model: str = "local-model") -> list[bytes]:
    """The reads of a streamed answer that says `pieces`, one event a read,
    then its end."""

    def event(delta: dict[str, str], finish: str | None = None) -> bytes:
        choice = {"index": 0, "delta": delta, "finish_reason": finish}
        chunk = {"id": "chatcmpl-1", "model": model, "choices": [choice]}
        return f"data: {json.dumps(chunk)}\n\n".encode()

    reads = [event({"content": piece}) for piece in pieces]
    return [*reads, event({}, "stop"), b"data: [DONE]\n\n"]


def call_events(name: str, arguments: str, model: str = "local-model") -> list[bytes]:
    """The reads of a streamed answer that calls the tool `name` with
    `arguments`, its JSON text, then its end."""
    function = {"name": name, "arguments": arguments}
    call = {"index": 0, "id": "call_1", "type": "function", "function": function}
    chunks = [
        {"index": 0, "delta": {"tool_calls": [call]}, "finish_reason": None},
        {"index": 0, "delta": {}, "finish_reason": "tool_calls"},
    ]
    reads = [
        f"data: {json.dumps({'id': 'chatcmpl-1', 'model': model, 'choices': [chunk]})}\n\n"
        for chunk in chunks
    ]
    return [*(read.encode() for read in reads), b"data: [DONE]\n\n"]


def local_config(port: int) -> dict[str, object]:
    """The members of the configuration of a provider that asks the fake on
    `port`. Its timeout is far past a test's deadline: a provider's own
    timeout never ends a wait in the place of what a test waits for."""
    return {
        "api_key": "placeholder-not-a-key",
        "model": "local-model",
        "base_url": f"http://127.0.0.1:{port}/v1",
        "timeout": 100 * DEADLINE,
    }
