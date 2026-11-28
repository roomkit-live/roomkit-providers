"""The cases ``generate_gemini_image_cases.py`` runs on RoomKit's Python Gemini
image provider: what it sends of an interaction (the prompt, the references,
the size as a ratio and a tier, the configuration's defaults and the request's
controls), how it reads an interaction back (its image, its usage, its steps),
the ``n`` interactions of a request of several images, what it refuses before
any request, and what it raises for a failure, the attempts it reports and the
images it keeps.

A request is written as ``openai_image_cases`` writes one.
"""

from __future__ import annotations

from typing import Any

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

BANANA = {"api_key": KEY}
PRO = {"api_key": KEY, "model": "gemini-3-pro-image"}
FLASH = {"api_key": KEY, "model": "gemini-3.1-flash-image"}
LITE = {"api_key": KEY, "model": "gemini-3.1-flash-lite-image"}
UNKNOWN = {"api_key": KEY, "model": "gemini-image-of-tomorrow"}

USAGE = {
    "total_input_tokens": 12,
    "input_tokens_by_modality": [{"modality": "text", "tokens": 12}],
    "total_output_tokens": 1300,
    "output_tokens_by_modality": [
        {"modality": "image", "tokens": 1290},
        {"modality": "text", "tokens": 10},
    ],
    "total_thought_tokens": 5,
}


def interaction(
    *images: dict[str, Any], text: str | None = "a paper fox", usage: Any = USAGE, **fields: Any
) -> dict[str, Any]:
    """An interaction the API answers: its last model output holds the
    image the SDK reads as ``output_image``, and its text."""
    content: list[dict[str, Any]] = []
    if text is not None:
        content.append({"type": "text", "text": text})
    content.extend(images or [image()])
    body: dict[str, Any] = {
        "id": "int_1",
        "status": "completed",
        "steps": [{"type": "model_output", "content": content}],
        **fields,
    }
    if usage is not None:
        body["usage"] = usage
    return body


def image(data: str | None = PNG, mime: str | None = "image/png", uri: str | None = None) -> dict[str, Any]:
    found: dict[str, Any] = {"type": "image"}
    if data is not None:
        found["data"] = data
    if mime is not None:
        found["mime_type"] = mime
    if uri is not None:
        found["uri"] = uri
    return found


def refused(status: int, message: str, kind: str) -> Reply:
    error = {"code": status, "message": message, "status": kind}
    return answer({"error": error}, status)


# -- A generation ----------------------------------------------------------------------

case("one_image", "generate answer", IMAGE_GENERATE, BANANA, asked(), answer(interaction(image(JPEG, "image/jpeg"))))
case("a_size_as_a_ratio_and_a_tier", "generate answer size", IMAGE_GENERATE, PRO, asked(size="1024x1024"), answer(interaction()))
case("a_wide_size", "generate answer size", IMAGE_GENERATE, FLASH, asked(size="2048x1152"), answer(interaction()))
case("a_size_of_a_ratio_the_model_lacks", "refused size", IMAGE_GENERATE, PRO, asked(size="1024x4096"))
case("a_size_past_the_largest_tier", "refused size", IMAGE_GENERATE, PRO, asked(size="8192x8192"))
case("a_size_of_a_tier_the_model_lacks", "refused size", IMAGE_GENERATE, PRO, asked(size="512x512"))
case(
    "the_configuration_s_defaults",
    "generate answer options",
    IMAGE_GENERATE,
    {**PRO, "image_size": "2K", "output_mime_type": "image/jpeg"},
    asked(),
    answer(interaction(image(JPEG, "image/jpeg"))),
)
case(
    "a_configured_type_the_model_lacks",
    "refused options",
    IMAGE_GENERATE,
    {**BANANA, "output_mime_type": "image/png"},
    asked(),
)
case(
    "a_size_over_the_configured_tier",
    "generate answer size options",
    IMAGE_GENERATE,
    {**PRO, "image_size": "4K"},
    asked(size="1536x1024"),
    answer(interaction()),
)
case(
    "the_controls",
    "generate answer options",
    IMAGE_WITH_OPTIONS,
    FLASH,
    asked(
        options={
            "aspect_ratio": "16:9",
            "image_size": "2K",
            "output_format": "jpeg",
            "thinking_level": "high",
            "store": False,
            "search_types": ["web_search", "image_search"],
        }
    ),
    answer(interaction(image(JPEG, "image/jpeg"))),
)
case(
    "a_continued_interaction",
    "generate answer options",
    IMAGE_WITH_OPTIONS,
    PRO,
    asked(options={"previous_interaction_id": "int_0", "store": True}),
    answer(interaction()),
)
case(
    "a_size_and_a_ratio",
    "refused options size",
    IMAGE_WITH_OPTIONS,
    PRO,
    asked(size="1024x1024", options={"aspect_ratio": "1:1"}),
)
case("a_control_the_model_lacks", "refused options", IMAGE_WITH_OPTIONS, PRO, asked(options={"thinking_level": "high"}))
case("a_mask", "refused", IMAGE_WITH_OPTIONS, PRO, asked(reference_images=[part()], mask=part()))
case("no_image", "refused", IMAGE_GENERATE, PRO, asked(n=0))
case("controls_on_an_unknown_model", "refused options", IMAGE_WITH_OPTIONS, UNKNOWN, asked(options={"image_size": "2K"}))
case(
    "a_size_on_an_unknown_model",
    "generate answer size",
    IMAGE_GENERATE,
    UNKNOWN,
    asked(size="1024x1024"),
    answer(interaction()),
)
case(
    "a_store_choice_on_an_unknown_model",
    "generate answer options",
    IMAGE_WITH_OPTIONS,
    UNKNOWN,
    asked(options={"store": False}),
    answer(interaction()),
)
case("progress_told", "generate answer progress", IMAGE_WITH_OPTIONS, PRO, asked(progress="record"), answer(interaction()))
case(
    "two_images",
    "generate answer progress",
    IMAGE_WITH_OPTIONS,
    PRO,
    asked(n=2, progress="record"),
    answer(interaction()),
    answer(interaction(image(JPEG, "image/jpeg"), id="int_2")),
)

# -- What an interaction says ----------------------------------------------------------

case("no_text_beside_the_image", "generate answer", IMAGE_GENERATE, PRO, asked(), answer(interaction(text=None)))
case("an_undeclared_type", "generate answer", IMAGE_GENERATE, PRO, asked(), answer(interaction(image(JPEG, None))))
case("no_usage_said", "generate answer usage", IMAGE_GENERATE, PRO, asked(), answer(interaction(usage=None)))
case(
    "a_usage_with_cached_tokens",
    "generate answer usage",
    IMAGE_GENERATE,
    PRO,
    asked(),
    answer(interaction(usage={**USAGE, "total_cached_tokens": 4})),
)
case(
    "a_usage_with_no_breakdown",
    "generate answer usage",
    IMAGE_GENERATE,
    PRO,
    asked(),
    answer(interaction(usage={"total_input_tokens": 12, "total_output_tokens": 1300})),
)
case(
    "an_image_after_a_thought",
    "generate answer",
    IMAGE_GENERATE,
    PRO,
    asked(),
    answer(
        {
            "id": "int_1",
            "status": "completed",
            "steps": [
                {"type": "thought", "content": [{"type": "text", "text": "thinking"}]},
                {"type": "model_output", "content": [{"type": "text", "text": "first"}]},
                {"type": "model_output", "content": [image(), {"type": "text", "text": "last"}]},
            ],
            "usage": USAGE,
        }
    ),
)
case(
    "an_interaction_with_no_image",
    "failure answer",
    IMAGE_GENERATE,
    PRO,
    asked(),
    answer({"id": "int_1", "status": "failed", "steps": [{"type": "model_output", "content": [{"type": "text", "text": "no"}]}]}),
)
case(
    "an_image_as_a_link",
    "failure answer",
    IMAGE_GENERATE,
    PRO,
    asked(),
    answer(interaction(image(None, "image/png", "https://example.com/fox.png"))),
)

# -- An interaction continued ----------------------------------------------------------
#
# An interaction cut at the decode limit answers ``requires_continuation`` with a
# token: the SDK continues it on its own, one hop after another, and merges the hops
# into one interaction.

USER_INPUT = {"type": "user_input", "content": [{"type": "text", "text": "an origami fox"}]}
FIRST_HOP_USAGE = {
    "total_input_tokens": 12,
    "input_tokens_by_modality": [{"modality": "text", "tokens": 12}],
    "total_output_tokens": 40,
    "output_tokens_by_modality": [{"modality": "text", "tokens": 40}],
}


def first_hop(token: str = "tok_1") -> dict[str, Any]:
    """The first hop of an interaction cut at the decode limit."""
    return {
        "id": "int_1",
        "status": "requires_continuation",
        "continuation_token": token,
        "steps": [USER_INPUT, {"type": "model_output", "content": [{"type": "text", "text": "drawing"}]}],
        "usage": FIRST_HOP_USAGE,
    }


def last_hop() -> dict[str, Any]:
    """The hop that completes it, repeating the input step."""
    body = interaction(id="int_2")
    body["steps"] = [USER_INPUT, *body["steps"]]
    return body


case(
    "a_generation_in_two_hops",
    "generate answer continuation",
    IMAGE_GENERATE,
    PRO,
    asked(),
    answer(first_hop()),
    answer(last_hop()),
)
case(
    "an_unstored_generation_in_two_hops",
    "generate answer continuation options",
    IMAGE_WITH_OPTIONS,
    PRO,
    asked(options={"store": False}),
    answer(first_hop()),
    answer(last_hop()),
)
case(
    "a_hop_that_hands_back_its_token",
    "failure answer continuation",
    IMAGE_GENERATE,
    PRO,
    asked(),
    answer(first_hop()),
    answer({**first_hop(), "id": "int_2"}),
)
case(
    "a_second_hop_refused",
    "failure status continuation progress",
    IMAGE_WITH_OPTIONS,
    PRO,
    asked(progress="record"),
    answer(first_hop()),
    refused(429, "Resource has been exhausted.", "RESOURCE_EXHAUSTED"),
)

# -- An edit ---------------------------------------------------------------------------

case(
    "an_edit_of_inline_references",
    "edit answer",
    IMAGE_GENERATE,
    PRO,
    asked("make it blue", reference_images=[part(), part(JPEG, "image/jpeg")]),
    answer(interaction()),
)
case(
    "a_reference_of_wrapped_base64",
    "edit answer",
    IMAGE_GENERATE,
    PRO,
    asked("make it blue", reference_images=[part(PNG[:20] + "\n" + PNG[20:])]),
    answer(interaction()),
)
case(
    "a_remote_reference",
    "edit answer",
    IMAGE_GENERATE,
    PRO,
    asked("make it blue", reference_images=[{"url": "https://example.com/fox.png", "mime_type": None}]),
    answer(interaction()),
)
case(
    "a_reference_that_is_no_base64",
    "refused",
    IMAGE_GENERATE,
    PRO,
    asked("make it blue", reference_images=[part("not*base64")]),
)
case(
    "too_many_references",
    "refused",
    IMAGE_GENERATE,
    {"api_key": KEY, "model": "gemini-2.5-flash-image"},
    asked("make it blue", reference_images=[part()] * 4),
)

# -- A failure -------------------------------------------------------------------------

for status, message, kind in [
    (400, "Image generation is not allowed for this prompt.", "INVALID_ARGUMENT"),
    (429, "Resource has been exhausted.", "RESOURCE_EXHAUSTED"),
    (500, "Internal error encountered.", "INTERNAL"),
    (503, "The model is overloaded.", "UNAVAILABLE"),
]:
    case(
        f"refused_with_{status}",
        "failure status progress",
        IMAGE_WITH_OPTIONS,
        PRO,
        asked(progress="record"),
        refused(status, message, kind),
    )
case("never_connected", "failure transport", IMAGE_GENERATE, PRO, asked(), REFUSED)
case(
    "lost_once_sent",
    "failure transport progress",
    IMAGE_WITH_OPTIONS,
    PRO,
    asked(progress="record"),
    half(interaction(), LOST),
)
case("timed_out_once_sent", "failure transport", IMAGE_GENERATE, PRO, asked(), half(interaction(), TIMEOUT))
case(
    "an_answer_that_is_no_json",
    "failure answer",
    IMAGE_GENERATE,
    PRO,
    asked(),
    Reply(status=200, headers={"content-type": "application/json"}, text="not json"),
)
case(
    "a_callback_that_fails",
    "failure progress",
    IMAGE_WITH_OPTIONS,
    PRO,
    asked(progress="fail"),
    answer(interaction()),
)
case(
    "the_second_of_two_fails",
    "failure status progress",
    IMAGE_WITH_OPTIONS,
    PRO,
    asked(n=2, progress="record"),
    answer(interaction()),
    refused(429, "Resource has been exhausted.", "RESOURCE_EXHAUSTED"),
)
