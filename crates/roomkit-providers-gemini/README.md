# roomkit-providers-gemini

The Gemini model provider of RoomKit Providers, on Google's Developer API.

- `Gemini`, a provider for Gemini's models: Python's `GeminiAIProvider`. It
  answers a request whole (`generate`), as a stream of events
  (`generate_structured_stream`) or of text (`generate_stream`), and lists
  the models its endpoint serves (`list_models`), every page of them;
- `GeminiConfig`, what the provider is configured with. It is the
  `GeminiConfig` of RoomKit's Python implementation: the same names, the same
  defaults, and the JSON it gives with `model_dump(mode="json")`, read and
  written;
- the wire cases the provider is held to
  (`tests/fixtures/python_gemini_cases.json`): what RoomKit's Python provider
  sends and reads, case by case, captured with `scripts/gemini_cases.py`.
  Every case is replayed against the provider through a local fake of the
  endpoint.

```rust
use roomkit_providers_gemini::{Gemini, GeminiConfig};

fn provider() -> Result<Gemini, roomkit_ai::ProviderError> {
    Gemini::new(GeminiConfig {
        thinking_level: Some("low".to_owned()),
        ..GeminiConfig::new("placeholder-not-a-key")
    })
}
```

## What it sends

- Every generation is asked as a stream,
  `models/{model}:streamGenerateContent?alt=sse`, a whole answer folded
  from it, the key in `x-goog-api-key`. A request is sent once: the SDK
  RoomKit's provider runs on tries none again.
- A turn's reasoning goes as Gemini's thinking config: the level of the
  model's catalogue nearest the turn's effort, else the configured
  `thinking_level`, else a positive budget, else `-1` (the model sizing it)
  for a turn that states on; for a turn that states off, a budget of `0`,
  or the lowest level on a model that cannot stop reasoning. A model the
  catalogue gives no level is sent no level.
- A round's calls go back with their thought signatures, the round's lent to
  a call Gemini left unsigned. A round of the current turn none of whose
  calls is signed, on a model that checks signatures, goes back as text, its
  results fenced, as the model reads a round another vendor answered.
- A tool's schema is declared as Gemini takes it: a union and a type list
  folded, the implied shape spelled out, an enum of no strings described,
  every key Gemini does not take dropped, `required` kept for the properties
  that survive. A name Gemini refuses (a leading digit) fails before any
  request.
- A history that ends with the model is refused before any request.

## What it reads

- Text and thoughts as they come; the calls folded across the chunks, a
  call re-emitted later folding into the first, two identical calls of one
  chunk staying two; the finish in the contract's words, a refusal under
  `refusal`, a blocked prompt under `prompt_block_reason`.
- Usage as the contract's disjoint counters: the input without what the
  cache held, the thoughts counted as output and told as
  `reasoning_tokens`; the input told once, as the stream starts.

## On Vertex AI

`GeminiVertex` is Python's `GeminiVertexProvider`, with `GeminiVertexConfig`:
the same generation, stream, thinking and catalogue, on a Google Cloud
project's region.

- A request goes to the host of the configuration's `location`, as the
  google-genai SDK writes it: `{location}-aiplatform.googleapis.com`,
  `aiplatform.googleapis.com` for `global`, `aiplatform.{us,eu}.rep.googleapis.com`
  for a multi-region; at `v1beta1`, the project and the location in the
  path; a model named as the SDK names it (`publishers/google/models/…`, a
  publisher's model, a resource given whole).
- It is sent as the identity google-auth would give, from `roomkit-auth`
  (its `google-cloud` feature): the key of `service_account_json`, its
  assertion signed as google-auth signs it; else Application Default
  Credentials (the file `GOOGLE_APPLICATION_CREDENTIALS` names, gcloud's
  file, the metadata server), a user's credentials among them; and the
  account `impersonate_service_account` names, borrowed with either. The
  token is kept until shortly before it expires; the project a file bills
  goes in `x-goog-user-project`. A token that cannot be had fails before
  the request, in the client's closed words.
- The billing labels go in every request, held to Google's rules when the
  configuration is read and when the provider is built.

```rust
use roomkit_providers_gemini::{GeminiVertex, GeminiVertexConfig};

fn provider(key_file: String) -> Result<GeminiVertex, roomkit_ai::ProviderError> {
    GeminiVertex::new(GeminiVertexConfig {
        service_account_json: Some(key_file),
        labels: Some(vec![("tenant".to_owned(), "acme".to_owned())]),
        ..GeminiVertexConfig::new("my-project", "northamerica-northeast1")
    })
}
```

Its wire cases (`tests/fixtures/python_vertex_cases.json`, captured with
`scripts/vertex_cases.py`) are replayed through a local fake of the token
endpoint and of Vertex AI, the assertion compared to its signature; the host
of each location is checked against the fixture apart, the fake standing in
for every host. Four cases are named: a token that cannot be had fails
before the request (`before_request`), where Python's google-auth error says
nothing of it.

An external account (`external_account`, workload identity federation)
and a GDCH service account are not taken yet; the first token fails.

## Image generation

`GeminiImage` is Gemini's image provider (RFC §25), RoomKit's Python
`GeminiImageProvider`, on the contract's `roomkit_ai::image::ImageProvider`.
Its configuration, `GeminiImageConfig`, is apart from the conversational one,
as Python's is: image models disjoint from the conversational ones, a tier and
a media type every request asks unless it asks its own, two minutes to answer
and five seconds to connect unless given. Its catalogue, `catalog/images.json`,
is Python's (`scripts/generate_gemini_catalogue.py`).

- An image is an interaction of the Interactions API (`POST
  v1beta/interactions`, the key in `x-goog-api-key`) with an image response
  format, written as Python's SDK writes it: `model` first, every other member
  in the order of its name. The prompt and the reference images are its
  input; inline references are decoded, which proves them, and sent again as
  canonical base64, remote ones sent as they are, for Google to fetch.
- A size (`WIDTHxHEIGHT`) is asked as Gemini's aspect ratio and resolution
  tier, the model's own where the catalogue lists it; it wins over the
  configured tier. The request's controls are merged over the
  configuration's, and the model's capabilities refuse what it cannot serve
  before anything is sent, in Python's words. A model the catalogue does not
  list takes no control but `store`.
- `n` images are `n` interactions, made at once and each settled alone: the
  first failure fails the generation, an `ImageGenerationError` that holds
  every attempt and the images another drew.
- One try: a paid interaction is never tried again on its own. An attempt is
  failed once an interaction came back or the vendor answered with a status,
  unknown otherwise (a connection lost or never made, a timeout, an answer
  that is no JSON), as Python's provider reads it.
- An interaction cut at the decode limit (`requires_continuation`, with a
  token) is continued hop after hop, as Python's SDK continues it: a hop
  continues the interaction that gave its token, or sends the input again
  when the request stores none. The hops are one interaction: the last
  one's id and status, the steps of every hop, the usage summed, the first
  hop's input kept as `initial_input_tokens`. A hop after the first that
  fails fails the attempt as an interrupted continuation, with no status: it
  is unknown. A server that asks to be continued for ever is continued 64
  hops; Python's SDK has no bound.
- The image is the last one of the model's outputs since the input, as the
  SDK reads `output_image`; an image sent as a link, or none, fails the
  attempt, final. Its type is the one declared, else the one its bytes show.
- The attempt keeps the interaction's steps, its trailing text and its usage
  as Python's provider keeps them: as the SDK dumps its models, their fields
  in the order the SDK declares them, payloads withheld
  (`catalog/interaction_fields.json`, written from the SDK by
  `scripts/generate_gemini_interaction_fields.py`). The usage is split as
  Python splits it: the image share of each direction apart from the text's,
  thought tokens with the text output, cached tokens unclassified.
- An attempt dropped while its interaction is in flight tells its callback it
  is unknown, as Python's provider does from its `CancelledError`, in a task
  of the runtime it went in flight on, wherever it is dropped. An answer is
  read to 128 MiB.

Python's SDK refuses no JSON answer: one its models do not validate is built
member by member, its defaults where a member is missing (an empty `id`, no
steps, no text), and one that is no object is kept as it is. The provider
reads an answer the same way, so a 200 that carries an error object is an
interaction with no image, failed. It takes an answer as validated from the
members it reads (`status` a text, `steps` a list of objects that each have
a `type`, `id`, `output_text`, `usage`, `output_image` of their types), where the
SDK checks every member: an answer that breaks a member the provider does not
read is validated here and not there, which changes only its `output_text`
when its steps hold no text (empty here, what the answer states there). An
answer of `null` is no interaction: its attempt is unknown.

Where it differs from Python's otherwise:

- A count of a usage past 64 bits saturates; Python's integers have no bound.
- An interrupted continuation quotes its hop's failure in the provider's
  words (`Error code: 429 - <message>`), where Python's quotes the SDK's
  repr of the server's body.
- A generation failed by its progress callback is raised from the
  `ImageGenerationError` that names the callback's failure, itself raised
  from it; Python's is raised from the callback's failure directly.

The provider is held to 63 cases captured on RoomKit's Python provider
(`tests/fixtures/python_image_cases.json`, written by
`scripts/generate_gemini_image_cases.py` from `scripts/gemini_image_cases.py`),
every one replayed as Python.

## Not covered

Gemini Live and the realtime voice stay RoomKit's Python's for now.

One case is named: a listing that fails raises the SDK's own error in
Python, which no provider reproduces; this one fails with a
`ProviderError`.
