"""The cases ``generate_openrouter_image_cases.py`` runs on RoomKit's Python
OpenRouter image provider: what it sends of a request to OpenRouter's Image API
(``POST /api/v1/images``: the prompt, the size as pixels, the configuration's
controls, the references as ``input_references``, the attribution headers), the
``n`` requests of a request of several images, how it reads an answer (its one
image, its media type, its usage and its cost), what it refuses before any
request, and what it raises for a failure.

A request is written as ``openai_image_cases`` writes one.
"""

from __future__ import annotations

import json

from openai_image_cases import JPEG, PNG, asked, part
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
)

CASES: list[Case] = []
case = registrar(CASES)

CONFIG = {"api_key": KEY, "model": "openai/gpt-image-2"}
ATTRIBUTED = {**CONFIG, "site_url": "https://example.com/app", "app_name": "An App"}
DEFAULTS = {**CONFIG, "quality": "high", "background": "transparent", "output_format": "webp"}
# The configured headers replace the attribution of the same name.
HEADERS = {**ATTRIBUTED, "default_headers": {"X-Title": "Another App", "X-Team": "imaging"}}
UNATTRIBUTED = {**CONFIG, "site_url": "", "app_name": ""}
ELSEWHERE = {**CONFIG, "base_url": "https://gateway.example/api/v1/"}

USAGE = {"prompt_tokens": 12, "completion_tokens": 4160, "total_tokens": 4172, "cost": 0.04}


def drawn(*images: dict, usage: object = USAGE, **fields: object) -> dict:
    """An answer of OpenRouter's Image API: one image."""
    body: dict = {"created": 1767225600, "data": list(images) or [image()], **fields}
    if usage is not None:
        body["usage"] = usage
    return body


def image(data: str | None = PNG, media_type: str | None = "image/png", **fields: object) -> dict:
    found: dict = {}
    if data is not None:
        found["b64_json"] = data
    if media_type is not None:
        found["media_type"] = media_type
    return {**found, **fields}


def refused(status: int, message: str) -> Reply:
    """A refusal, its body as text: the provider repeats it word for word,
    so both fakes serve the same bytes."""
    body = json.dumps({"error": {"code": status, "message": message}})
    return Reply(status=status, headers={"content-type": "application/json"}, text=body)


# -- A generation ----------------------------------------------------------------------

case("one_image", "generate answer", IMAGE_GENERATE, CONFIG, asked(), answer(drawn()))
case("a_size", "generate answer size", IMAGE_GENERATE, CONFIG, asked(size="1536x1024"), answer(drawn()))
case(
    "a_size_written_loosely",
    "generate answer size",
    IMAGE_GENERATE,
    CONFIG,
    asked(size=" 1024X1024 "),
    answer(drawn()),
)
case("a_size_that_is_no_size", "refused size", IMAGE_GENERATE, CONFIG, asked(size="large"))
case(
    "the_configuration_s_controls",
    "generate answer options",
    IMAGE_GENERATE,
    DEFAULTS,
    asked(),
    answer(drawn(image(JPEG, "image/webp"))),
)
case("attribution_headers", "generate answer", IMAGE_GENERATE, ATTRIBUTED, asked(), answer(drawn()))
case("default_headers_over_attribution", "generate answer", IMAGE_GENERATE, HEADERS, asked(), answer(drawn()))
case("an_empty_attribution", "generate answer", IMAGE_GENERATE, UNATTRIBUTED, asked(), answer(drawn()))
case("another_endpoint", "generate answer", IMAGE_GENERATE, ELSEWHERE, asked(), answer(drawn()))
case(
    "two_images",
    "generate answer",
    IMAGE_GENERATE,
    CONFIG,
    asked(n=2),
    answer(drawn()),
    answer(drawn(image(JPEG, "image/jpeg", revised_prompt="a paper fox"))),
)
case("no_image", "refused", IMAGE_GENERATE, CONFIG, asked(n=0))
case("too_many_images", "refused", IMAGE_GENERATE, CONFIG, asked(n=11))
case(
    "ten_images",
    "generate answer",
    IMAGE_GENERATE,
    CONFIG,
    asked(n=10),
    *(answer(drawn()) for _ in range(10)),
)

# -- What the controls of the image surface ask ----------------------------------------

case(
    "the_controls_of_another_entry",
    "refused options",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(options={"quality": "low"}),
)
case("a_mask", "refused", IMAGE_WITH_OPTIONS, CONFIG, asked(reference_images=[part()], mask=part()))
case("a_progress_callback", "refused progress", IMAGE_WITH_OPTIONS, CONFIG, asked(progress="record"))
case(
    "with_options_and_none_given",
    "generate answer",
    IMAGE_WITH_OPTIONS,
    CONFIG,
    asked(),
    answer(drawn()),
)

# -- What an answer says ---------------------------------------------------------------

case("an_undeclared_type", "generate answer", IMAGE_GENERATE, CONFIG, asked(), answer(drawn(image(JPEG, None))))
case("no_usage_said", "generate answer usage", IMAGE_GENERATE, CONFIG, asked(), answer(drawn(usage=None)))
case(
    "a_usage_with_no_cost",
    "generate answer usage",
    IMAGE_GENERATE,
    CONFIG,
    asked(),
    answer(drawn(usage={"prompt_tokens": 12, "completion_tokens": 4160})),
)
case(
    "a_usage_written_as_fractions",
    "generate answer usage",
    IMAGE_GENERATE,
    CONFIG,
    asked(),
    answer(drawn(usage={"prompt_tokens": 12.7, "completion_tokens": 4160.2, "cost": 0.04})),
)
case(
    "a_usage_that_is_no_object",
    "generate answer usage",
    IMAGE_GENERATE,
    CONFIG,
    asked(),
    answer(drawn(usage=[1, 2])),
)
case("a_revised_prompt", "generate answer", IMAGE_GENERATE, CONFIG, asked(), answer(drawn(image(revised_prompt="a paper fox"))))
case("no_image_answered", "failure answer", IMAGE_GENERATE, CONFIG, asked(), answer({"created": 1, "data": []}))
case("two_images_answered", "failure answer", IMAGE_GENERATE, CONFIG, asked(), answer(drawn(image(), image())))
case("an_image_without_bytes", "failure answer", IMAGE_GENERATE, CONFIG, asked(), answer(drawn(image(None))))
case("an_answer_that_is_no_object", "failure answer", IMAGE_GENERATE, CONFIG, asked(), answer([1, 2]))
case(
    "an_answer_that_is_no_json",
    "failure answer",
    IMAGE_GENERATE,
    CONFIG,
    asked(),
    Reply(status=200, headers={"content-type": "application/json"}, text="not json"),
)

# -- An edit ---------------------------------------------------------------------------

case(
    "an_edit_of_inline_references",
    "edit answer",
    IMAGE_GENERATE,
    CONFIG,
    asked("make it blue", reference_images=[part(), part(JPEG, "image/jpeg")]),
    answer(drawn()),
)
case(
    "a_reference_of_wrapped_base64",
    "edit answer",
    IMAGE_GENERATE,
    CONFIG,
    asked("make it blue", reference_images=[part(PNG[:20] + "\n" + PNG[20:])]),
    answer(drawn()),
)
case(
    "a_remote_reference",
    "edit answer",
    IMAGE_GENERATE,
    CONFIG,
    asked("make it blue", reference_images=[{"url": "https://example.com/fox.png", "mime_type": None}]),
    answer(drawn()),
)
case(
    "a_reference_that_is_no_base64",
    "refused",
    IMAGE_GENERATE,
    CONFIG,
    asked("make it blue", reference_images=[part("not*base64")]),
)

# -- A failure -------------------------------------------------------------------------

for status in (400, 402, 429, 500, 502, 503):
    case(
        f"refused_with_{status}",
        "failure status",
        IMAGE_GENERATE,
        CONFIG,
        asked(),
        refused(status, "The routed model refused the request."),
    )
case(
    "a_refusal_of_a_long_body",
    "failure status",
    IMAGE_GENERATE,
    CONFIG,
    asked(),
    Reply(status=400, headers={"content-type": "text/plain"}, text="x" * 800),
)
case("never_connected", "failure transport", IMAGE_GENERATE, CONFIG, asked(), REFUSED)
case("lost_once_sent", "failure transport", IMAGE_GENERATE, CONFIG, asked(), half(drawn(), LOST))
case("timed_out_once_sent", "failure transport", IMAGE_GENERATE, CONFIG, asked(), half(drawn(), TIMEOUT))
case(
    "the_second_of_two_fails",
    "failure status",
    IMAGE_GENERATE,
    CONFIG,
    asked(n=2),
    answer(drawn()),
    refused(429, "Rate limit exceeded."),
)
