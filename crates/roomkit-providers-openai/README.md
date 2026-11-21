# roomkit-providers-openai

The OpenAI model provider of RoomKit Providers.

**Early development.** The crate holds today:

- `OpenAiConfig`, what a provider for OpenAI's Chat Completions is configured
  with. It is the `OpenAIConfig` of RoomKit's Python implementation: the same
  names, the same defaults, and the JSON it gives with
  `model_dump(mode="json")`, read and written;
- the wire cases a provider will have to replay
  (`tests/fixtures/python_chat_cases.json`): what RoomKit's Python provider
  sends and reads on Chat Completions, case by case.

The provider itself is not written yet.

```rust
use roomkit_providers_openai::OpenAiConfig;

fn main() -> serde_json::Result<()> {
    let config = OpenAiConfig {
        base_url: Some("http://localhost:11434/v1".to_owned()),
        ..OpenAiConfig::new("placeholder-not-a-key", "local-model")
    };
    // The key is written masked, as Python writes it.
    let written = serde_json::to_string(&config)?;
    assert!(written.contains(r#""api_key":"**********""#));
    Ok(())
}
```

It depends on `roomkit-providers-core`, and on neither Python nor RoomKit.

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
- A base URL that Python's URL reader refuses (a bracket that holds no IPv6
  address, a character that normalizes to a delimiter) makes Python refuse
  the configuration. Here the configuration is a plain value: it is not
  OpenAI's endpoint, and nothing is refused.

The `Debug` form never prints the key, nor the base URL's user information,
path or query, nor a default header's value.

## The wire cases

`tests/fixtures/python_chat_cases.json` is written by
`scripts/generate_openai_chat_cases.py`, which runs RoomKit's Python
`OpenAIAIProvider` against a fake of the endpoint, with no network. A case
holds the configuration, the request in the contract's JSON form, each HTTP
request Python sent with the answer the fake gave, and what Python returned:
a response, the events or the text of a stream, or the error it raised. The
fixture's `notes` say how each part is written.

The crate's tests read every case with `OpenAiConfig` and the contract's
types, and require every kind of case: a regenerated fixture that loses one
fails.

## License

MIT
