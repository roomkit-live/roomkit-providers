# roomkit-providers-openai

The OpenAI model provider of RoomKit Providers.

**Early development.** The crate holds today:

- `OpenAiAiProvider`, a provider for OpenAI's Chat Completions and for the
  servers that speak it behind a base URL. It answers a request whole
  (`generate`); its streams are not written yet;
- `OpenAiConfig`, what the provider is configured with. It is the
  `OpenAIConfig` of RoomKit's Python implementation: the same names, the same
  defaults, and the JSON it gives with `model_dump(mode="json")`, read and
  written;
- the wire cases the provider is held to
  (`tests/fixtures/python_chat_cases.json`): what RoomKit's Python provider
  sends and reads on Chat Completions, case by case. Every case of a whole
  answer is replayed against the provider through a local fake of the
  endpoint.

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

## What the provider guarantees

- **The key.** It appears in no failure, log, `Debug` form or result. A
  failure that echoes a server's answer has the key removed from it.
- **Sizes.** An answer past 4 MiB fails the request. What a failure echoes of
  a server's answer is on one line, within 500 characters.
- **Where a request goes.** To the base URL of the configuration, OpenAI's
  own when it names none, followed by `/chat/completions`. A redirect is not
  followed: the request fails. A base URL with a query or a fragment is
  refused when the provider is built.
- **The environment.** Nothing is read from it: no key, no base URL, no
  proxy. The client asks for no compressed answer.
- **Time.** `connect_timeout` bounds a connection, `timeout` the wait between
  two reads of an answer. A request is tried again only as `max_retries`
  allows, which is 0 unless the configuration says.

## Where the provider differs from Python's

The provider does as `OpenAIAIProvider` of RoomKit's Python implementation on
183 of the 203 captured cases of a whole answer. It differs where Python's
behaviour is the openai SDK's or is questioned, each for a reason:

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
  zero) stays unknown, where Python converts some and fails on others with an
  error of its own.
- **A failure's message** says what the server answered, as Python's:
  `Error code: <status> - <body>` for a JSON body, the body itself for any
  other text. The body is written as JSON, where Python writes its `repr`;
  and it is cleaned as said above, where Python keeps it whole. A request
  that got no answer fails in this crate's words, where Python has the
  SDK's.
- **`supports_vision`** is the configuration's `vision`, where Python reads
  its catalogue of models; **`list_models`** is the catalogue, where Python
  asks the endpoint.
- **Whether the endpoint is OpenAI's own**, which decides the rules of its
  models, is Python's comparison of the base URL, and also what the client
  dials: both must say so.

The rules of a try, which Python leaves to the openai SDK, are the SDK's: a
connection that failed, a timeout, an answer that says `x-should-retry`, or a
status of 408, 409, 429 or 500 and above is tried again, after the wait
`retry-after-ms` or `retry-after` asks within two minutes, else 0.5 s
doubling to 8 s, up to a quarter less.

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
and what Python returned: a response, the events or the text of a stream, or
the error it raised. The fixture's `notes` say how each part is written.

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
fails. Every case of a whole answer is replayed against the provider: the
requests it sends, the tries and the waits, and the response or the failure
it reads are compared with Python's. A case the provider answers otherwise
is named in the test with its reason and with what the provider does, which
is asserted; a named case that starts to replay as Python fails the test.

## License

MIT
