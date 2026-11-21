# roomkit-providers-openai

The OpenAI model provider of RoomKit Providers.

**Early development.** The crate holds today:

- `OpenAiAiProvider`, a provider for OpenAI's Chat Completions and for the
  servers that speak it behind a base URL. It answers a request whole
  (`generate`), as a stream of events (`generate_structured_stream`) or of
  text (`generate_stream`), and lists the models its endpoint serves
  (`list_models`);
- `OpenAiConfig`, what the provider is configured with. It is the
  `OpenAIConfig` of RoomKit's Python implementation: the same names, the same
  defaults, and the JSON it gives with `model_dump(mode="json")`, read and
  written;
- the wire cases the provider is held to
  (`tests/fixtures/python_chat_cases.json`): what RoomKit's Python provider
  sends and reads on Chat Completions and on the model listing, case by
  case. Every case is replayed against the provider through a local fake of
  the endpoint.

```rust
use roomkit_providers_core::{AiMessage, AiProvider, AiRequest, AiRole};
use roomkit_providers_openai::{OpenAiAiProvider, OpenAiConfig};

async fn ask() -> Result<String, roomkit_providers_core::ProviderError> {
    let config = OpenAiConfig {
        base_url: Some("http://localhost:11434/v1".to_owned()),
        ..OpenAiConfig::new("placeholder-not-a-key", "local-model")
    };
    let provider = OpenAiAiProvider::new(config)?;
    let request = AiRequest {
        messages: vec![AiMessage::new(AiRole::User, "Hello")],
        ..AiRequest::default()
    };
    Ok(provider.generate(&request).await?.content)
}
```

`examples/local_server.rs` asks once whole and once streamed, against a
stand-in server it starts on a local port, and checks what it read:

```sh
cargo run -p roomkit-providers-openai --example local_server
```

## Streams

`generate_structured_stream` gives the events of one generation as they
come: the text and the reasoning (a `<think>` block that opens the answer,
or a reasoning field beside the text), the composition of each call
(`ToolCallDelta`, under the id the call ends with), then the whole calls and
the done event, with the usage and the model that answered.
`generate_stream` gives the text alone.

- **A stream is pulled.** Nothing is sent before the stream is first polled,
  and the answer is read only as its events are taken: a reader that stops
  taking them stops the reading.
- **Dropping the stream stops the request.** Its connection is closed,
  whatever was left of the answer. Nothing runs in the background.
- **A failure is the last item.** What was read before it was handed on
  first. A request is tried again, as `max_retries` allows, until the head of
  its answer came, never after: a stream that fails under way fails, and
  `retryable` says whether a new request may succeed.
- **An answer under a response schema** is checked before its done event,
  which its failure replaces. The text went out as it came: it is
  provisional until the done event.
- **The answer is read as a stream of events** whatever the server says it
  is. Lines end with a line feed, a carriage return, or both. A page or a
  whole JSON body holds no event, which is a failure.

## The model listing

`list_models` asks `GET <base URL>/models`, with the headers, the timeouts
and the tries of any request. The page carries ids alone: what the crate's
catalogue knows of a model (its name, its context window, its price) is
filled from it, a speech model is tagged, and a model the catalogue does not
know passes through with its id. A body that is no page of models with
their ids is a failure, never an empty listing. `available_models` is the
catalogue, with no request.

## What the provider guarantees

- **The key.** It appears in no failure, log, `Debug` form or result. A
  failure that echoes a server's answer has the key removed from it.
- **What a failure echoes.** Of a server's answer, only what an error object
  says of itself: its message, its type and its code, on one line, within
  500 characters. A body that is no JSON (a gateway's page, a plain text) is
  never echoed.
- **Sizes.** Every size a server controls is bounded. An answer read whole
  past 4 MiB fails the request. A stream fails at a line past 1 MiB, at an
  event past 1 MiB, and past 256 MiB in all; each is a final failure, after
  the events read before it.
- **Where a request goes.** To the base URL of the configuration, OpenAI's
  own when it names none, followed by `/chat/completions` or `/models`. A
  redirect is not followed: the request fails. A base URL with a query or a
  fragment is refused when the provider is built.
- **The environment.** Nothing is read from it: no key, no base URL, no
  proxy. The client asks for no compressed answer.
- **Time.** `connect_timeout` bounds a connection, `timeout` the wait between
  two reads of an answer, not the whole answer: a stream may last longer,
  and one that stalls ends at `timeout`, a failure worth a retry. A request
  is tried again only as `max_retries` allows, which is 0 unless the
  configuration says.

### What a failed request says

A request the server refuses fails with its status, in these words:

| The server answered | The failure says |
| --- | --- |
| `401` with `{"error": {"message": "Incorrect API key provided.", "type": "invalid_request_error", "code": "invalid_api_key"}}` | `the provider answered HTTP 401 (invalid_request_error, invalid_api_key): Incorrect API key provided.` |
| `502` with an HTML page | `the provider answered HTTP 502: Bad Gateway` |
| `500` with an empty body | `the provider answered HTTP 500: Internal Server Error` |

The message, the type and the code are read in the body's `error` object,
or at its top level. `status_code` holds the status, `retryable` whether it
is one a new request may get past (408, 409, 429, 500 and above), and
`context_overflow` is `true` when the code is `context_length_exceeded`.

## Where the provider differs from Python's

The provider does as `OpenAIAIProvider` of RoomKit's Python implementation on
286 of the 326 captured cases: 181 of the 203 of a whole answer, 97 of the
110 of a stream of events, the 4 of a stream of text, and 4 of the 9 of a
model listing. It differs on the others, each for a reason:

- **Redirects** are refused, where Python's SDK follows them, a `301`, a
  `302` or a `303` as a `GET` without the request's body. A request of a
  model's turn is not sent elsewhere than where the configuration says.
- **A base URL with a query or a fragment** is refused when the provider is
  built, where Python sends to a path written after the query.
- **An empty key** builds a provider that sends no `Authorization`, for a
  local server that asks for none, where Python's SDK refuses to build a
  client.
- **A count of tokens** is read when it is a JSON whole number, a float with
  no fraction included. Any other (a text, `true`, a fraction, a number below
  zero) stays unknown, whole or streamed, where Python converts some and
  fails on others, with an error of its own on a whole answer.
- **A failure's message** for a status is the provider's own sentence, as
  the table above shows, where Python's carries the whole body the server
  answered. So a body that is no JSON is not echoed, where Python's message
  is that body; and a refusal for a context too long that a server words
  outside its error's message is not in the text for a host to read. A
  request that got no answer fails in this crate's words, where Python has
  the SDK's.
- **The last event of a stream** that ends with no blank line after it is
  read, where Python's SDK drops it.
- **An event that holds no data** (a named event a server keeps a connection
  alive with) is no event, as the format says, where Python's SDK reads it
  as JSON and fails the stream.
- **A stream is bounded**, as said above, where Python's SDK reads a line or
  an event of any length.
- **A call's index** in a stream is a whole number; another fails the
  stream, where Python takes any value as the call's key.
- **`list_models`** fails with a `ProviderError`, as a completion does, where
  Python's raises the openai SDK's error or a raw exception as it is: a
  status, a connection that failed, a body that is no page of models.
- **`supports_vision`** is the configuration's `vision`, where Python reads
  its catalogue of models.
- **Whether the endpoint is OpenAI's own**, which decides the rules of its
  models, is Python's comparison of the base URL, and also what the client
  dials: both must say so.

A member of an answer written in another shape than the dialect's (a usage
that is a text, a delta that is no object) reads as absent. What Python
fails a stream on fails it here, in this crate's words: a chunk that is no
object, choices or calls that are no list, a text, a finish reason or a
call's arguments of the wrong type.

The rules of a try, which Python leaves to the openai SDK, are the SDK's: a
connection that failed, a timeout, an answer that says `x-should-retry`, or a
status of 408, 409, 429 or 500 and above is tried again, after the wait
`retry-after-ms` or `retry-after` asks within two minutes, else 0.5 s
doubling to 8 s, up to a quarter less. A whole answer is read inside its
try; a stream is tried until its head came.

It depends on `roomkit-providers-core` and `roomkit-providers-http`, and on
neither Python nor RoomKit.

## Where the configuration differs from Python's

- `timeout` and `connect_timeout` are a `Duration`, where Python holds
  seconds as a float. The JSON form writes seconds, as Python's; a timeout is
  kept to the nanosecond, and one below zero is refused.
- `use_max_completion_tokens` and `supports_custom_temperature` are
  `Option<bool>`: `None` is a switch left unset, which takes the default of
  the model on OpenAI's own endpoint, as Python decides it when the
  configuration is built. The JSON form writes the switch decided, `true` or
  `false`, as Python does, and a member left out of what is read leaves it
  unset.
- `vision` is this crate's: Python reads whether a model takes images from
  its catalogue of models. It is no part of the JSON form.
- `default_headers` are written sorted by name.
- Input is stricter: a value pydantic would convert (`"30"` for a number,
  `1024.0` for an integer, `1` for a boolean) and an integer outside 32 bits
  without a sign are refused.
- A member written twice in the JSON text is refused, where Python keeps the
  last, and an integer of `extra_body` outside 64 bits is read as a float:
  both come from the JSON parser.
- A base URL that Python's URL reader refuses (a bracket that holds no IPv6
  address, a character that normalizes to a delimiter) makes Python refuse
  the configuration. Here the configuration is a plain value: it is not
  OpenAI's endpoint, and nothing is refused.

The `Debug` form never prints the key, nor the base URL's user information,
path or query, nor a default header's value. The JSON form is not for
showing: like Python's, it masks the key alone, and writes a default
header's value and the credentials a base URL carries whole.

## The wire cases

`tests/fixtures/python_chat_cases.json` is written by
`scripts/generate_openai_chat_cases.py`, which runs RoomKit's Python
`OpenAIAIProvider` against a fake of the endpoint, with no network. A case
holds the configuration, the request in the contract's JSON form, each HTTP
request Python sent, with the URL it went to, and the answer the fake gave,
and what Python returned: a response, the events or the text of a stream,
the models of a listing, or the error it raised. The fixture's `notes` say
how each part is written.

Three members say how a replay takes a case:

- `compare`, on a case that fails: what a replay reproduces of the failure
  as written (its attributes, the reason of a response schema's failure, and
  what a stream gave first), and whose words its message holds, so whether
  it is compared. A provider with no SDK under it cannot say what the openai
  SDK, Python's JSON reader or Python's `repr` of a body says;
- `sdk`: the openai SDK decided the behaviour, not RoomKit;
- `questioned`: the case pins a behaviour of Python that is not settled, in
  a sentence that starts with its kind: a possible defect, reported
  upstream, or a difference to decide, where the Rust provider is planned
  otherwise and no defect is claimed. A replay may set the case aside, or
  expect otherwise once it is settled.

The crate's tests read every case with `OpenAiConfig` and the contract's
types, and require every kind of case: a regenerated fixture that loses one
fails. Every case is replayed against the provider: the requests it sends,
the tries and the waits, and what it gives are compared with Python's: the
response, the events in their order, the text, the models, and a failure
with what the stream gave before it. The fake serves a stream read by read,
each read the bytes of one recorded chunk, so the provider sees the cuts
Python saw. An id the provider mints for a call is compared by its shape. A
case the provider answers otherwise is named in the test with its reason and
with what the provider does, which is asserted; a named case that starts to
replay as Python fails the test.

## License

MIT
