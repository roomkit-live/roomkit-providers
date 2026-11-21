"""The contract's values as Python holds them: plain dicts in the JSON form.

A request is written, and an answer read, in the JSON form of the contract,
the one RoomKit's models give with ``model_dump(mode="json")``. These types
name the members of those dicts for a type checker; nothing checks them when
the program runs, and the provider reads a request with the contract's own
reader, which refuses what does not fit.

A member a request may leave out is ``NotRequired``: it then takes the
default of the contract. What a provider gives back always holds every
member.
"""

# No postponed annotations here: a typed dict reads `NotRequired` when its
# class is made, and cannot read it in an annotation kept as text.
from typing import Any, Literal, NotRequired, TypedDict

ToolCallOutcome = Literal["served", "refused", "failed", "blocked", "unserved", "cancelled"]


class AITextPart(TypedDict):
    """A piece of text in a message or in a tool's result."""

    type: Literal["text"]
    text: str


class AIImagePart(TypedDict):
    """An image, by its URL: an ``https`` one, or a ``data:`` URI."""

    type: Literal["image"]
    url: str
    mime_type: NotRequired[str | None]


class AIThinkingPart(TypedDict):
    """A block of a model's reasoning, as its provider gave it."""

    type: Literal["thinking"]
    thinking: str
    signature: NotRequired[str | None]
    redacted: NotRequired[str | None]


class AIToolCallPart(TypedDict):
    """A call a model made, as an earlier round of the turn holds it."""

    type: Literal["tool_call"]
    id: str
    name: str
    arguments: NotRequired[dict[str, Any]]
    metadata: NotRequired[dict[str, Any]]


class AIToolResultPart(TypedDict):
    """What a tool answered a call."""

    type: Literal["tool_result"]
    tool_call_id: str
    name: str
    result: str | list[AITextPart | AIImagePart]
    is_error: NotRequired[bool]
    structured_content: NotRequired[dict[str, Any] | None]
    outcome: NotRequired[ToolCallOutcome | None]
    references: NotRequired[list[str]]


AIContentPart = AITextPart | AIImagePart | AIToolCallPart | AIToolResultPart | AIThinkingPart


class AIMessage(TypedDict):
    """One message of a conversation."""

    role: Literal["system", "user", "assistant", "tool"]
    content: str | list[AIContentPart]
    metadata: NotRequired[dict[str, Any]]


class AITool(TypedDict):
    """A tool the model may call."""

    name: str
    description: str
    parameters: NotRequired[dict[str, Any]]
    tags: NotRequired[list[str]]
    defer_loading: NotRequired[bool]
    changes_state: NotRequired[bool]


class AIRequest(TypedDict, total=False):
    """What one generation is asked: the members a provider reads of
    RoomKit's ``AIContext``. Every member may be left out.

    ``temperature`` is 0.7 when left out. ``response_schema`` is a JSON
    Schema within the contract's portable subset: a provider handed one
    answers with one JSON document satisfying it, or fails with a
    ``ResponseSchemaError``.
    """

    messages: list[AIMessage]
    system_prompt: str | None
    temperature: float
    max_tokens: int | None
    thinking_budget: int | None
    enable_thinking: bool | None
    reasoning_effort: str | None
    tools: list[AITool]
    response_schema: dict[str, Any] | None
    metadata: dict[str, Any]


class ServedCall(TypedDict):
    """What a provider's own service answered a call it served itself."""

    result: str
    is_error: bool


class AIToolCall(TypedDict):
    """A call the model made: ``partial`` when it must not run."""

    id: str
    name: str
    arguments: dict[str, Any]
    metadata: dict[str, Any]
    served: ServedCall | None
    partial: bool
    garbled: bool
    intent: str | None


class AIResponse(TypedDict):
    """One whole answer."""

    content: str
    thinking: str | None
    thinking_signature: str | None
    thinking_parts: list[AIThinkingPart] | None
    finish_reason: str | None
    usage: dict[str, int]
    metadata: dict[str, Any]
    tool_calls: list[AIToolCall]


class StreamTextDelta(TypedDict):
    """A piece of the answer's text, as it came."""

    type: Literal["text_delta"]
    text: str


class StreamThinkingDelta(TypedDict):
    """A piece of the model's reasoning, as it came."""

    type: Literal["thinking_delta"]
    thinking: str
    signature: str | None
    block: int | None
    redacted: str | None


class StreamToolCallDelta(TypedDict):
    """A piece of a call's arguments, under the id the call ends with."""

    type: Literal["tool_call_delta"]
    id: str
    name: str
    index: int
    arguments_delta: str


class StreamToolCall(TypedDict):
    """A whole call, once its arguments are read."""

    type: Literal["tool_call"]
    id: str
    name: str
    arguments: dict[str, Any]
    metadata: dict[str, Any]
    served: ServedCall | None
    partial: bool
    garbled: bool
    intent: str | None


class StreamUsage(TypedDict):
    """The usage so far, from a provider that reports it under way."""

    type: Literal["usage"]
    usage: dict[str, int]


class StreamDone(TypedDict):
    """The end of a stream: why it stopped, what it used, who answered."""

    type: Literal["done"]
    finish_reason: str | None
    usage: dict[str, int]
    metadata: dict[str, Any]


StreamEvent = (
    StreamTextDelta
    | StreamThinkingDelta
    | StreamToolCallDelta
    | StreamToolCall
    | StreamUsage
    | StreamDone
)


class ModelPricing(TypedDict):
    """What a model costs, per million tokens; ``verified`` is a day,
    ``YYYY-MM-DD``."""

    input_per_million: float
    output_per_million: float
    cache_read_per_million: float | None
    cache_write_per_million: float | None
    image_input_per_million: float | None
    image_output_per_million: float | None
    long_context_threshold_tokens: int | None
    long_context_input_multiplier: float
    long_context_output_multiplier: float
    currency: str
    verified: str


class ModelInfo(TypedDict):
    """What is known of a model: its id, and what a catalogue adds."""

    id: str
    display_name: str | None
    context_window: int | None
    supports_vision: bool | None
    deprecated: bool
    capabilities: list[str]
    pricing: ModelPricing | None
