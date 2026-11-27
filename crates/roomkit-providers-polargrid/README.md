# roomkit-providers-polargrid

The PolarGrid model provider of RoomKit Providers, on the chat completions
of a PolarGrid edge.

- `PolarGrid`, a provider for the models of a PolarGrid edge: Python's
  `PolarGridAIProvider`. It answers a request whole (`generate`), as a
  stream of events (`generate_structured_stream`) or of text
  (`generate_stream`), and lists the models its edge serves
  (`list_models`). It names the edges PolarGrid runs
  (`PolarGrid::available_regions`) and the one it sends its requests to
  (`connected_region`), as Python's provider does;
- `PolarGridConfig`, what the provider is configured with. It is the
  `PolarGridConfig` of RoomKit's Python implementation: the same names, the
  same defaults, and the JSON it gives with `model_dump(mode="json")`, read
  and written; `PolarGridRegion` is an edge, as Python's `RegionInfo`;
- the wire cases the provider is held to
  (`tests/fixtures/python_polargrid_cases.json`): what RoomKit's Python
  provider sends and reads, case by case, captured with
  `scripts/polargrid_cases.py`. Every case is replayed against the provider
  through a local fake of the edge and of the autorouter.

```rust
use roomkit_providers_polargrid::{PolarGrid, PolarGridConfig};

fn provider() -> Result<PolarGrid, roomkit_ai::ProviderError> {
    PolarGrid::new(PolarGridConfig {
        region: Some("Montreal".to_owned()),
        ..PolarGridConfig::new("placeholder-not-a-key")
    })
}
```

## Where it sends

- A configuration that pins a `region` (an edge's id, or a name such as
  `Montreal` or `toronto`, in any case) sends to that edge.
- One that pins none asks the autorouter, once, at the first call, for the
  nearest edge that serves the model: its pick is kept for as long as it
  says, a day when it says nothing, in a cache the whole process shares.
  An autorouter that answers no pick, whatever the reason, sends the
  request to polargrid-sdk's default edge, `yto-01`; that pick is not kept.

## What it sends

- A whole answer is asked of `/v1/chat/completions`, the key as a bearer
  token; an empty key is read from `POLARGRID_API_KEY`, as the SDK reads
  it. It is tried again only when it got no answer, after one second, then
  two, up to `max_retries`; a refused request is never tried again. A
  stream is tried once.
- The body is written as polargrid-sdk writes it: its members in the SDK's
  order, each message as the SDK's `Message` writes it back. The
  conversation is RoomKit's chat rendering, text sent flat, each tool
  message naming its tool. A stream asks for its usage.
- The output bound is the turn's, else the configured one, at most 4,096
  (the SDK's bound); with neither, what the model's window leaves after the
  prompt, counted at a token every three characters.
- A turn's thinking switch goes as `enable_thinking`, over the configured
  one. A response schema goes strict, as `response_format`; one asked
  beside tools is refused before any request, as are a temperature past 0
  to 2 and a `top_p` past 0 to 1.

## What it reads

- An answer as the SDK reads it: one that lacks what the SDK requires (an
  id, a date, a model, choices, a usage with its three counts, a known
  finish reason, a call's id) fails.
- Text, and reasoning in `<think>` tags; the calls, folded across the
  chunks of a stream; the finish in the contract's words, a content filter
  read as a refusal of a constrained answer.
- In a stream, a payload that is no JSON is skipped, and so is a chunk the
  SDK does not read as one; a payload that names an `error` fails the
  stream, retryable by its words. The usage the edge sends last is read
  apart.
- A refused request reads as the SDK's class of it: a 403 as a 401, a
  status the SDK has no class for as none, retryable by its words.

## Not covered

The Python binding leaves `available_regions` and `connected_region` out
for now: a binding gives a provider's contract only; the methods a vendor
adds beside it wait for a mechanism every vendor can use.

No case is named: every case replays as Python.
