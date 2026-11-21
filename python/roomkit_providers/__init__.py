"""Model providers implemented in Rust, for asynchronous Python programs.

A provider is used without RoomKit: it takes a request and gives its answer
as plain dicts in the contract's JSON form, which the types here name.

    from roomkit_providers.openai import OpenAIAIProvider, OpenAIConfig
"""

from ._contract import (
    AIContentPart,
    AIImagePart,
    AIMessage,
    AIRequest,
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
    StreamEvent,
    StreamTextDelta,
    StreamThinkingDelta,
    StreamToolCall,
    StreamToolCallDelta,
    StreamUsage,
    ToolCallOutcome,
)
from ._errors import ProviderError, ResponseSchemaError, ResponseSchemaFailure

__version__ = "0.1.0a1"

__all__ = [
    "AIContentPart",
    "AIImagePart",
    "AIMessage",
    "AIRequest",
    "AIResponse",
    "AITextPart",
    "AIThinkingPart",
    "AITool",
    "AIToolCall",
    "AIToolCallPart",
    "AIToolResultPart",
    "ModelInfo",
    "ModelPricing",
    "ProviderError",
    "ResponseSchemaError",
    "ResponseSchemaFailure",
    "ServedCall",
    "StreamDone",
    "StreamEvent",
    "StreamTextDelta",
    "StreamThinkingDelta",
    "StreamToolCall",
    "StreamToolCallDelta",
    "StreamUsage",
    "ToolCallOutcome",
    "__version__",
]
