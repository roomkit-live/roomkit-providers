"""The cases ``generate_openai_image_cases.py`` runs on RoomKit's Python OpenAI
image provider: what it sends for a generation (the prompt, the size, the
count, the controls, the configuration's defaults) and for an edit (the
references and the mask, uploaded as multipart), how it reads an answer and a
stream of previews, what it refuses before any request, and what it raises
for a failure, the attempt it reports and the images it keeps.

A request is written as the image surface takes it: ``prompt``, ``size``,
``n``, ``reference_images`` and ``mask`` (``AIImagePart`` dumps), ``options``
(``ImageOptions`` keywords) and ``progress``: none, ``record`` (a callback
that records every attempt it is told of) or ``fail`` (one that records it,
then raises when told of a succeeded attempt).
"""

from __future__ import annotations

import base64
import json
from typing import Any

from wire_capture import (
    IMAGE_GENERATE,
    IMAGE_WITH_OPTIONS,
    KEY,
    LOST,
    REFUSED,
    TIMEOUT,
    Case,
    Reply,
    answer,
    half,
    registrar,
    stream,
)

CASES: list[Case] = []
case = registrar(CASES)

# A real one-pixel PNG, and a JPEG head: what an endpoint returns, small.
PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="
JPEG = base64.b64encode(b"\xff\xd8\xff\xe0 the rest of a jpeg").decode()

CONFIG = {"api_key": KEY, "model": "gpt-image-2"}
MINI = {"api_key": KEY, "model": "gpt-image-1-mini"}
# A model that keeps its references as closely as asked (input_fidelity).
FAITHFUL = {"api_key": KEY, "model": "gpt-image-1.5"}
# A deployment no catalogue entry describes, as a user names an Azure one.
UNKNOWN = {"api_key": KEY, "model": "my-image-deployment"}
DEFAULTS = {**CONFIG, "quality": "high", "background": "opaque", "output_format": "webp"}

REQUEST_ID = {"x-request-id": "req_image_1"}

USAGE = {
    "input_tokens": 50,
    "input_tokens_details": {"image_tokens": 40, "text_tokens": 10},
    "output_tokens": 1056,
    "total_tokens": 1106,
}


def asked(prompt: str = "an origami fox", **given: Any) -> dict[str, Any]:
    return {
        "prompt": prompt,
        "size": None,
        "n": 1,
        "reference_images": [],
        "options": None,
        "mask": None,
        "progress": None,
        **given,
    }


def part(data: str = PNG, mime: str = "image/png") -> dict[str, Any]:
    return {"url": f"data:{mime};base64,{data}", "mime_type": mime}


def drawn(*images: dict[str, Any], **fields: Any) -> dict[str, Any]:
    """An answer of the images API."""
    return {
        "created": 1767225600,
        "data": list(images) or [{"b64_json": PNG}],
        "background": "opaque",
        "output_format": "png",
        "quality": "high",
        "size": "1024x1024",
        "usage": USAGE,
        **fields,
    }


def image(data: str = PNG, revised_prompt: str | None = None) -> dict[str, Any]:
    found: dict[str, Any] = {"b64_json": data}
    if revised_prompt is not None:
        found["revised_prompt"] = revised_prompt
    return found


def failed(status: int, message: str, kind: str = "invalid_request_error") -> Reply:
    error = {"message": message, "type": kind, "param": None, "code": None}
    return answer({"error": error}, status, **REQUEST_ID)


def sse(kind: str, **fields: Any) -> str:
    return "data: " + json.dumps({"type": kind, **fields}) + "\n\n"


def previews(kind: str, count: int, *, usage: bool = True) -> list[str]:
    """A stream of ``count`` previews then the final image, as the images
    API streams a generation (``image_generation``) or an edit
    (``image_edit``)."""
    common = {"background": "opaque", "output_format": "png", "quality": "high", "size": "1024x1024"}
    events = [
        sse(f"{kind}.partial_image", b64_json=PNG, partial_image_index=at, created_at=1767225600, **common)
        for at in range(count)
    ]
    final = {"b64_json": PNG, "created_at": 1767225600, **common}
    if usage:
        final["usage"] = USAGE
    events.append(sse(f"{kind}.completed", **final))
    return events


# -- A generation ----------------------------------------------------------------------

case("one_image", "generate answer", IMAGE_GENERATE, CONFIG, asked(), answer(drawn(), **REQUEST_ID))
case(
    "a_revised_prompt",
    "generate answer",
    IMAGE_GENERATE,
    CONFIG,
    asked(),
    answer(drawn(image(revised_prompt="a paper fox, folded")), **REQUEST_ID),
)
case(
    "two_images_at_a_size",
    "generate answer size",
    IMAGE_GENERATE,
    CONFIG,
    asked(size="1536X1024", n=2),
    answer(drawn(image(), image(JPEG), size="1536x1024"), **REQUEST_ID),
)
case(
    "an_auto_size",
    "generate answer size",
    IMAGE_GENERATE,
    MINI,
    asked(size="auto"),
    answer(drawn(size="1024x1536"), **REQUEST_ID),
)
case(
    "the_controls",
    "generate answer options",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(
        options={
            "quality": "low",
            "background": "transparent",
            "output_format": "webp",
            "output_compression": 70,
            "moderation": "low",
        }
    ),
    answer(drawn(output_format="webp", background="transparent", quality="low"), **REQUEST_ID),
)
case(
    "the_configuration_s_defaults",
    "generate answer options",
    IMAGE_GENERATE,
    DEFAULTS,
    asked(),
    answer(drawn(output_format="webp"), **REQUEST_ID),
)
case(
    "an_option_over_a_default",
    "generate answer options",
    IMAGE_WITH_OPTIONS,
    DEFAULTS,
    asked(options={"quality": "medium", "output_format": "jpeg", "background": "auto"}),
    answer(drawn(output_format="jpeg", quality="medium"), **REQUEST_ID),
)
case(
    "no_format_said",
    "generate answer",
    IMAGE_GENERATE,
    CONFIG,
    asked(),
    answer({key: value for key, value in drawn().items() if key != "output_format"}, **REQUEST_ID),
)
case(
    "no_usage_said",
    "generate answer usage",
    IMAGE_GENERATE,
    CONFIG,
    asked(),
    answer({key: value for key, value in drawn().items() if key != "usage"}),
)
case(
    "a_usage_with_no_details",
    "generate answer usage",
    IMAGE_GENERATE,
    CONFIG,
    asked(),
    answer(drawn(usage={"input_tokens": 30, "output_tokens": 1056, "total_tokens": 1086})),
)
case(
    "a_usage_with_no_text_count",
    "generate answer usage",
    IMAGE_GENERATE,
    CONFIG,
    asked(),
    answer(drawn(usage={"input_tokens": 50, "input_tokens_details": {"image_tokens": 40}, "output_tokens": 1056})),
)
case(
    "a_format_asked_not_said",
    "generate answer options",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(options={"output_format": "webp"}),
    answer({key: value for key, value in drawn().items() if key != "output_format"}, **REQUEST_ID),
)
case(
    "progress_told",
    "generate answer progress",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(progress="record"),
    answer(drawn(), **REQUEST_ID),
)

# -- An edit ---------------------------------------------------------------------------

case(
    "an_edit_of_one_reference",
    "edit answer multipart",
    IMAGE_GENERATE,
    CONFIG,
    asked("make it blue", reference_images=[part()]),
    answer(drawn(), **REQUEST_ID),
)
case(
    "an_edit_of_ordered_references",
    "edit answer multipart",
    IMAGE_GENERATE,
    CONFIG,
    asked("put them together", reference_images=[part(), part(JPEG, "image/jpeg")]),
    answer(drawn(), **REQUEST_ID),
)
case(
    "a_mask_and_a_moderation",
    "edit answer multipart options",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(
        "paint the sky",
        reference_images=[part()],
        mask=part(),
        options={"moderation": "low", "input_fidelity": None},
    ),
    answer(drawn(), **REQUEST_ID),
)

case(
    "a_faithful_edit",
    "edit answer multipart options",
    IMAGE_WITH_OPTIONS,
    FAITHFUL,
    asked("make it blue", reference_images=[part()], options={"input_fidelity": "high"}),
    answer(drawn(), **REQUEST_ID),
)

# -- A stream of previews --------------------------------------------------------------

case(
    "previews_then_the_image",
    "stream progress",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(options={"partial_images": 2}, progress="record"),
    Reply(status=200, headers={"content-type": "text/event-stream", **REQUEST_ID}, chunks=previews("image_generation", 2)),
)
case(
    "previews_of_an_edit",
    "stream progress multipart",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked("make it blue", reference_images=[part()], options={"partial_images": 1}, progress="record"),
    Reply(status=200, headers={"content-type": "text/event-stream", **REQUEST_ID}, chunks=previews("image_edit", 1)),
)
case(
    "a_stream_cut_after_its_image",
    "stream failure progress",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(options={"partial_images": 1}, progress="record"),
    stream(previews("image_generation", 1), cut=LOST),
)

STREAMED = {"content-type": "text/event-stream", **REQUEST_ID}
case(
    "a_stream_error_after_its_image",
    "stream failure progress",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(options={"partial_images": 1}, progress="record"),
    Reply(
        status=200,
        headers=STREAMED,
        chunks=[*previews("image_generation", 1), "data: " + json.dumps({"error": {"message": "boom from the stream"}}) + "\n\n"],
    ),
)
case(
    "a_stream_error_before_any_image",
    "stream failure progress",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(options={"partial_images": 1}, progress="record"),
    Reply(status=200, headers=STREAMED, chunks=["data: " + json.dumps({"error": {"message": "boom from the stream"}}) + "\n\n"]),
)
case(
    "a_typed_stream_error",
    "stream failure progress",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(options={"partial_images": 1}, progress="record"),
    Reply(status=200, headers=STREAMED, chunks=[sse("error", message="the image could not be drawn")]),
)
case(
    "an_event_that_is_no_json",
    "stream failure progress",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(options={"partial_images": 1}, progress="record"),
    Reply(status=200, headers=STREAMED, chunks=[*previews("image_generation", 1)[:1], "data: not json\n\n"]),
)
case(
    "done_before_its_image",
    "stream failure progress",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(options={"partial_images": 1}, progress="record"),
    Reply(status=200, headers=STREAMED, chunks=["data: [DONE]\n\n", *previews("image_generation", 1)]),
)
case(
    "a_completed_image_with_no_bytes",
    "stream failure progress",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(options={"partial_images": 1}, progress="record"),
    Reply(status=200, headers=STREAMED, chunks=[sse("image_generation.completed", output_format="png", usage=USAGE)]),
)
case(
    "an_empty_revised_prompt",
    "generate answer",
    IMAGE_GENERATE,
    CONFIG,
    asked(),
    answer(drawn(image(revised_prompt="")), **REQUEST_ID),
)

# -- Refused before any request --------------------------------------------------------

case("too_many_images", "refused", IMAGE_GENERATE, CONFIG, asked(n=11))
case("no_image", "refused", IMAGE_GENERATE, CONFIG, asked(n=0))
case("a_size_unread", "refused size", IMAGE_GENERATE, CONFIG, asked(size="huge"))
case("a_size_the_model_does_not_draw", "refused size", IMAGE_GENERATE, MINI, asked(size="2048x2048"))
case(
    "a_control_the_model_does_not_take",
    "refused options",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(options={"thinking_level": "high"}),
)
case(
    "transparent_jpeg",
    "refused options",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(options={"background": "transparent", "output_format": "jpeg"}),
)
case(
    "a_fidelity_the_model_does_not_take",
    "refused options",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(options={"input_fidelity": "high"}),
)
case(
    "a_fidelity_with_no_reference",
    "refused options",
    IMAGE_WITH_OPTIONS,
    FAITHFUL,
    asked(options={"input_fidelity": "high"}),
)
case(
    "previews_with_no_callback",
    "refused options",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(options={"partial_images": 1}),
)
case(
    "a_mask_that_is_no_png",
    "refused multipart",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked("paint the sky", reference_images=[part()], mask=part(JPEG, "image/jpeg")),
)
case(
    "a_media_type_with_line_breaks",
    "refused multipart",
    IMAGE_GENERATE,
    CONFIG,
    asked("make it blue", reference_images=[{"url": f"data:;base64,{PNG}", "mime_type": "image/png\r\nX-Injected: 1"}]),
)
case(
    "a_remote_reference",
    "refused multipart",
    IMAGE_GENERATE,
    CONFIG,
    asked("make it blue", reference_images=[{"url": "https://example.com/fox.png", "mime_type": "image/png"}]),
)
case(
    "controls_on_an_unknown_model",
    "refused options",
    IMAGE_WITH_OPTIONS,
    UNKNOWN,
    asked(options={"moderation": "low"}),
)
case("too_many_images_of_an_unknown_model", "refused", IMAGE_GENERATE, UNKNOWN, asked(n=11))
case(
    "an_unknown_model_keeps_its_defaults",
    "generate answer options",
    IMAGE_GENERATE,
    {**UNKNOWN, "quality": "high"},
    asked(),
    answer(drawn(), **REQUEST_ID),
)

# -- A failure -------------------------------------------------------------------------

for status, message in [
    (400, "Your request was rejected by the safety system."),
    (429, "Rate limit reached for images."),
    (500, "The server had an error while processing your request."),
    (503, "The engine is currently overloaded."),
]:
    case(
        f"refused_with_{status}",
        "failure status progress",
        IMAGE_WITH_OPTIONS,
        CONFIG,
        asked(progress="record"),
        failed(status, message),
    )
case("never_connected", "failure transport", IMAGE_GENERATE, CONFIG, asked(), REFUSED)
case(
    "lost_once_sent",
    "failure transport progress",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(progress="record"),
    half(drawn(), LOST),
)
case("timed_out_once_sent", "failure transport", IMAGE_GENERATE, CONFIG, asked(), half(drawn(), TIMEOUT))
case(
    "fewer_images_than_asked",
    "failure answer",
    IMAGE_GENERATE,
    CONFIG,
    asked(n=2),
    answer(drawn(), **REQUEST_ID),
)
case(
    "an_image_with_no_bytes",
    "failure answer",
    IMAGE_GENERATE,
    CONFIG,
    asked(n=2),
    answer(drawn(image(), {"url": "https://example.com/fox.png"}), **REQUEST_ID),
)
case(
    "a_callback_that_fails",
    "failure progress",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(progress="fail"),
    answer(drawn(), **REQUEST_ID),
)
