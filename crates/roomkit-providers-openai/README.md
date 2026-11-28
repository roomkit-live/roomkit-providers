# roomkit-providers-openai

The OpenAI model provider of RoomKit Providers.

**Early development.** The crate holds today:

- `OpenAi`, a provider for OpenAI's Chat Completions, for its Responses
  API and for the servers that speak either behind a base URL. It answers a request whole
  (`generate`), as a stream of events (`generate_structured_stream`) or of
  text (`generate_stream`), and lists the models its endpoint serves
  (`list_models`);
- `OpenAiConfig`, what the provider is configured with. It is the
  `OpenAIConfig` of RoomKit's Python implementation: the same names, the same
  defaults, and the JSON it gives with `model_dump(mode="json")`, read and
  written;
- `DeepSeek`, `Kimi`, `MiniMax`, `Xai`, `Cerebras`, `Qwen`, `Meta`,
  `OpenRouter` and `LiteLlm`, the providers of nine vendors that speak Chat
  Completions with rules of their own, each with its configuration (see
  [Nine vendors on the same wire](#nine-vendors-on-the-same-wire));
- the wire cases the provider is held to
  (`tests/fixtures/python_chat_cases.json`): what RoomKit's Python provider
  sends and reads on Chat Completions and on the model listing, case by
  case. Every case is replayed against the provider through a local fake of
  the endpoint. The nine vendors are held the same way to the cases
  captured on their own Python providers
  (`tests/fixtures/python_<vendor>_cases.json`, from
  `scripts/vendor_chat_cases.py`), and their configurations to the JSON
  forms of Python's (`tests/fixtures/python_<vendor>_config_forms.json`).

```rust
use roomkit_ai::{AiMessage, AiProvider, AiContext, AiRole};
use roomkit_providers_openai::{OpenAi, OpenAiConfig};

async fn ask() -> Result<String, roomkit_ai::ProviderError> {
    let config = OpenAiConfig {
        base_url: Some("http://localhost:11434/v1".to_owned()),
        ..OpenAiConfig::new("placeholder-not-a-key", "local-model")
    };
    let provider = OpenAi::new(config)?;
    let request = AiContext {
        messages: vec![AiMessage::new(AiRole::User, "Hello")],
        ..AiContext::default()
    };
    Ok(provider.generate(&request).await?.content)
}
```

`examples/local_server.rs` asks once whole and once streamed, against a
stand-in server it starts on a local port, and checks what it read:

```sh
cargo run -p roomkit-providers-openai --example local_server
```

`examples/behind_a_proxy.rs` asks three servers from behind a proxy, with
nothing of the proxy in the configuration: the provider reads
`HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY` and `SSL_CERT_FILE` from its
environment. The proxy, the servers and the authority that signs the
certificate of one of them are stand-ins on local ports, and the example
checks which way each request went:

```sh
cargo run -p roomkit-providers-openai --example behind_a_proxy
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
  is. Lines end with a line feed, a carriage return, or both, and a byte
  order mark that starts the answer is ignored. A page or a whole JSON body
  holds no event, which is a failure; a JSON error object in place of a
  stream fails as the status it names, as it does for `generate`.
- **A stream is complete once it said why it stopped, or sent its end
  mark.** One that ends with neither was cut: what came is handed on, then
  it fails, a failure worth a retry, and a call it brought is never handed
  whole, to run. A finish reason that is an empty text
  (`"finish_reason": ""`) says nothing: it is no reason, and no stop.
- **How an answer ended is told in the contract's word** (`FinishReason`),
  as Python tells it: `stop`, `length`, `content_filter` and `error` as
  themselves, `tool_calls` and the older `function_call` as `tool_calls`.
  A word the dialect does not hold, a server's own or one that is no text,
  is no reason; the stream that said it is complete all the same. On the
  Responses API, a response cut by its output cap, or cut with no reason
  given, ends as `length`, and one a filter stopped as `content_filter`.
- **An answer that ends inside an event was cut there.** The event an answer
  ends on, with no blank line after it, is read when its data is a whole
  JSON document, or the end mark, with or without a line end after its last
  line. When its data is neither, the answer ended inside it: nothing of
  that event is read, and the stream is what it was before it: complete if
  it had said why it stopped, cut if it had not, and an answer that carried
  no event if that event was its first. The line end cannot tell the two
  apart, since a whole last line may lack its own; the data can, since an
  event's data is a JSON object and no start of one is a JSON document. A
  character the answer's end cuts is part of the cut.

## The Responses API

`OpenAiConfig::api` chooses the HTTP API the provider speaks: Chat
Completions by default, `OpenAiApi::Responses` for `POST <base
URL>/responses`. The choice is explicit: no failure makes the provider try
the other one.

```rust
use roomkit_providers_openai::{OpenAi, OpenAiApi, OpenAiConfig};

fn provider() -> Result<OpenAi, roomkit_ai::ProviderError> {
    OpenAi::new(OpenAiConfig {
        api: OpenAiApi::Responses,
        reasoning_effort: Some("medium".to_owned()),
        ..OpenAiConfig::new("placeholder-not-a-key", "a-reasoning-model")
    })
}
```

- **A request is stateless**: `store` is false and no response id is sent.
  The system prompt goes as `instructions`, the conversation as `input`.
- **A request is always streamed**, and `generate` is its stream read to
  its end. Text and reasoning come as they are written. A call is handed on
  only by the terminal event of the answer: an answer that ends before it,
  or fails, runs no tool.
- **Inside one tool loop, an answer goes back as it came**: its output
  items, the encrypted reasoning among them, are kept in the answer's
  metadata and sent back with the tools' results, with the ids and the
  arguments RoomKit gave the calls. What cannot go back as it came is
  refused before any request.
- **`tool_namespace`** declares the tools under one namespace, its name and
  its description, and a call the server makes in another one fails the
  answer.
- **The reasoning effort** goes as `reasoning.effort`, as it is stated.

RoomKit's Python implementation has no Responses API: this one is the
Rust core's, and the wire cases do not cover it.

## Nine vendors on the same wire

`DeepSeek`, `Kimi`, `MiniMax`, `Xai`, `Cerebras`, `Qwen`, `Meta`,
`OpenRouter` and `LiteLlm` are the providers of RoomKit's Python
implementation for these vendors (`DeepSeekAIProvider`, `KimiAIProvider`,
`MiniMaxAIProvider`, `XAIAIProvider`, `CerebrasAIProvider`,
`QwenAIProvider`, `MetaAIProvider`, `OpenRouterAIProvider`,
`LiteLLMAIProvider`).
Each is `OpenAi` with its vendor's rules: every request and answer is Chat
Completions', read and bounded as `OpenAi`'s are. Its configuration
(`DeepSeekConfig`, `KimiConfig`, `MiniMaxConfig`, `XaiConfig`,
`CerebrasConfig`, `QwenConfig`, `MetaConfig`, `OpenRouterConfig`,
`LiteLlmConfig`) has `OpenAiConfig`'s members with its vendor's endpoint
and defaults, and Cerebras's, Qwen's and OpenRouter's members of their
own.

```rust
use roomkit_providers_openai::{Kimi, KimiConfig};

fn provider() -> Result<Kimi, roomkit_ai::ProviderError> {
    // On Kimi's own API; `base_url` points at a server serving its weights.
    Kimi::new(KimiConfig::new("placeholder-not-a-key", "kimi-k3"))
}
```

What a vendor's provider does that `OpenAi` does not:

| | DeepSeek | Kimi | MiniMax |
| --- | --- | --- | --- |
| A round's earlier reasoning goes back in | `reasoning_content`, on every round that called tools, empty for one that did not reason | `reasoning_content`, on every round that called tools, a single space for one that did not reason | `reasoning_content`, on a round that reasoned |
| A reasoning setting goes as | a `thinking` object, its effort nested in it, never a top-level `reasoning_effort` | `reasoning_effort`, the nearest of `low`, `high`, `max` on a model of its catalogue | `reasoning_effort`, the nearest of `low` to `max` on a model of its catalogue; `thinking: {"type": "disabled"}` on M3 for a turn that states off |
| On a turn with tools, the setting | is sent | is sent | is sent |
| Tool names on its own endpoint | `[A-Za-z0-9_-]{1,128}` | `[a-zA-Z_][a-zA-Z0-9_-]{0,127}` | not checked |
| A response schema, unless the configuration says | refused before the request | taken | refused before the request |
| Token bound by default | 1024, as `max_tokens` | none; one that is set goes as `max_completion_tokens` | none; one that is set goes as `max_completion_tokens` |
| Temperature by default | sent | not sent | not sent |
| Usage | cache hits read from `prompt_cache_hit_tokens`, not billed as input | as `OpenAi` | as `OpenAi` |

| | xAI | Cerebras | Qwen | Meta |
| --- | --- | --- | --- | --- |
| A round's earlier reasoning goes back in | the field the configuration names, else nowhere | `reasoning`, on a round that reasoned | the field the configuration names, else nowhere | the field the configuration names, else nowhere |
| A reasoning setting goes as | `reasoning_effort`, withheld from a model its catalogue lists without `thinking` | `reasoning_effort`, beside the configuration's `reasoning_format` (`parsed` by default) and `clear_thinking` | `enable_thinking`, and `thinking_budget` on a turn that thinks; no effort | `reasoning_effort`; `minimal` for a turn that states off |
| On a turn with tools, the setting | is sent | is sent | is sent | is sent |
| Tool names on its own endpoint | not checked | not checked | not checked | not checked |
| A response schema, unless the configuration says | taken | taken | refused before the request | taken |
| Token bound by default | 1024, as `max_completion_tokens` | 1024, as `max_completion_tokens` | 1024, as `max_tokens` | 1024, as `max_completion_tokens` |
| Temperature by default | sent | sent | sent | sent |
| Usage | reasoning reported beside the completion counts as output | as `OpenAi`; a stream asks for none by default | as `OpenAi` | as `OpenAi` |
| `list_models` | the endpoint's | the endpoint's | its catalogue, as Model Studio lists none | the endpoint's chat models (`muse-spark`, `muse-wonder`, `muse-ember`) |

| | OpenRouter | LiteLLM |
| --- | --- | --- |
| A round's earlier reasoning goes back in | the field the configuration names, else nowhere | the field the configuration names, else nowhere |
| A reasoning setting goes as | a `reasoning` object: `{"enabled": false}` for a turn that states off, a budget as its `max_tokens`, else the effort, else `{"enabled": true}` for a turn that states on | `reasoning_effort`, a budget as a `thinking` object; nothing for a turn that states off, as no off survives every upstream |
| On a turn with tools, the setting | is sent | is sent where `supports_reasoning_effort_with_tools` is true; left out otherwise, with a warning once when it is unset |
| Tool names on its own endpoint | not checked | not checked |
| A response schema, unless the configuration says | taken, the request routed only where every parameter is honoured (`provider.require_parameters`) | taken |
| Token bound by default | 1024, as `max_tokens` | 1024, as `max_tokens` |
| Temperature by default | sent | sent |
| Usage | as `OpenAi` | as `OpenAi` |
| `list_models` | the routed models, with their names, windows and whether they read images (`architecture.input_modalities`) | the proxy's `/model/info`: one model per public name, its deployments folded into what all deliver, with the proxy's prices |

- **A model the vendor's catalogue does not name** is sent what the turn
  states as it is, and reads images when it is served behind another base
  URL, where the server decides. DeepSeek's provider reads such a model's
  name as `OpenAi` does.
- **A turn that states off** (a thinking budget of 0, `enable_thinking`
  false, or an effort of `none`) is sent the least its model takes: `low`
  on the models of Kimi's catalogue and on MiniMax-M3.1-Flash, which cannot
  stop reasoning, the floor of Cerebras's catalogue entry (`low` on
  `gpt-oss-120b`), `minimal` on every Muse chat model. DeepSeek and
  MiniMax-M3 are sent `thinking: {"type": "disabled"}`, Qwen
  `enable_thinking: false`.
- **A failure names its vendor** (`deepseek`, `kimi`, `minimax`, `xai`,
  `cerebras`, `qwen`, `meta`, `openrouter`, `litellm`), and `list_models`
  fills what its vendor's catalogue knows of a model.
- **`supports_reasoning_effort_with_tools` is refused** when the provider
  is built, but by LiteLLM's, and so is `thinking_field` by DeepSeek's,
  Kimi's, MiniMax's and Cerebras's: the vendor's own rule decides. The
  others take a thinking field, for a server of one's own; LiteLLM's takes
  the setting of a turn with tools, the model behind a proxy's name being
  unknown.
- **OpenRouter's attribution** goes as `HTTP-Referer` (`site_url`) and
  `X-Title` (`app_name`), an empty one left out, under the configuration's
  default headers, which win over one of the same name however it is
  written.
- **LiteLLM's prices are dated** the day the listing is read, by the
  provider's clock, in UTC; Python's provider dates them in the machine's
  local time, a day apart around midnight.
- **xAI's and Meta's configurations name a model** for a form that leaves it
  out (`grok-4.7`, `muse-spark-1.3`), as Python's do. Meta's refuses a key
  that is not ASCII, which no header can carry, and never says the key.
- **Cerebras's arrays are read back**: an array argument the model wrote as
  the JSON text of the array is read as the array where the tool's schema
  declares one, in a whole answer and in a stream's whole calls.
- **An Azure deployment** (`AzureAi`, `AzureAiConfig`: Python's
  `AzureAIProvider`, `AzureAIConfig`) is asked at
  `{endpoint}/openai/deployments/{deployment}/chat/completions`, its listing
  at `{endpoint}/openai/models`, each with the configuration's
  `api-version`, the key in `api-key` and no bearer. Its configuration is
  no `OpenAiConfig`: an `azure_endpoint` and an `api_version` in place of a
  base URL, no default headers and no thinking field. The endpoint is
  reduced to the resource's base as Python's `normalize_azure_endpoint`
  does: the v1 API's URL, a deployment's target URI, a query, a fragment
  and trailing slashes go, an API Management gateway's `/openai` prefix
  stays. A reasoning setting goes on a turn with tools only where
  `supports_reasoning_effort_with_tools` says so, a response schema beside
  tools unless the configuration says otherwise, and images to any
  deployment. A call with an empty key is refused before any request with
  a `ProviderError`, where Python's openai SDK raises its own
  `OpenAIError` (`Missing credentials`).
- **A vLLM server** (`Vllm`, `VllmConfig`: what Python's
  `create_vllm_provider(VLLMConfig)` builds) is asked as `OpenAi` asks a
  server behind a base URL, its key `none` by default and its `headers`
  sent as default headers. Its configuration is no `OpenAiConfig`: vLLM's
  sampling members (`top_p`, `top_k`, `min_p`, `presence_penalty`,
  `repetition_penalty`) ride the body, under the configuration's
  `extra_body`; its `enable_thinking` and `reasoning_effort` ride the chat
  template's `chat_template_kwargs`, under the extra body's own, and a
  turn's switch and effort (but `none`) over them, on a turn with tools as
  on any other. No top-level `reasoning_effort` is sent: a local template
  does not read it.
- A vendor's provider speaks Chat Completions only, with the
  configuration's key: it has no `api`, no `token_source` and no `vision`.

Each is held to the cases captured on its Python provider
(`tests/fixtures/python_<vendor>_cases.json`), every one replayed as
Python, and its configuration to the JSON forms of Python's.

## A source of tokens

`OpenAiConfig::token_source` names where the bearer token of a request
comes from, in place of `api_key`: an implementation of `TokenSource`, the
host's. It is asked before each request and before each try of one. A
source that gives no token fails the request before anything is sent, in
the words of its `TokenError`; the key is never sent in a token's place.
The token is removed from what the provider reads of the answer, so that no
failure repeats it. An `Authorization` among `default_headers` replaces the
token as it replaces the key, and the source is then not asked.

## The model listing

`list_models` asks `GET <base URL>/models`, with the headers, the timeouts
and the tries of any request. The page carries ids alone: what the crate's
catalogue knows of a model (its name, its context window, its price) is
filled from it, a speech model is tagged, and a model the catalogue does not
know passes through with its id. A body that is no page of models with
their ids is a failure, never an empty listing. `available_models` is the
catalogue, with no request.

## What the provider guarantees

- **Secrets.** No failure's text repeats a secret of the configuration that
  a server's answer holds: the key, the value of each default header (and,
  where the value names a scheme such as `Bearer`, what follows it), and the
  user information of the base URL, as it is written, as it reads once
  decoded, and as the `Authorization` the client writes from it. Nor does
  the failure of a generation repeat the key a host lends its request, under
  `api_key` in the request's metadata: the provider does not send that key,
  here as in Python, and still holds it for a secret of the request. Each is
  replaced by `[***]`. This holds whatever built the failure: the provider,
  the contract's check of a response schema, or a reader. A secret is found
  as the server sent it and as the code that builds a failure writes it
  again, with its quotes, its backslashes and its other characters escaped:
  as a JSON string, as a reader quotes a text it refuses, and as the
  contract quotes a value. It is found as a URL writes it too, a character
  as it is or as a percent sign and two hexadecimal digits, whichever
  characters the server wrote so. A character that shows nothing is dropped
  from a failure's text before the secrets are looked for, so one put
  inside a secret does not hide it. Two limits:
  - a secret shorter than 8 characters is not searched for: it would be
    found in ordinary words;
  - a secret a server itself sends back in another form (encoded another
    way, cut in pieces, in another case) cannot be recognised;
  - a member of `extra_body` is no secret to a failure: a server that
    repeats one in the message of its refusal is repeated.

  The `Debug` forms print none of them, and the provider logs none.
- **What a failure repeats of an answer.** Only this, each on one line and
  bounded:
  - of a request the server refuses, what its error object says of itself:
    its message, its type and its code (a type or a code within 64
    characters), or, where it states no message, what it words under
    `detail`: a text, or of a list each entry's `msg` and the place its
    `loc` names. A body that is no JSON (a gateway's page, a plain text) is
    never repeated;
  - of an answer that holds an error object in place of a completion, the
    object's message, or the object itself, written as JSON, when it has
    none; of an error a stream carries, its message;
  - of an answer a reader cannot read, the reader's words, which may quote
    one value: a call's id that is no text, or the member the JSON reader
    refuses (`invalid type: string "...", expected u64`);
  - of an answer a response schema refuses, the contract's sentence, which
    quotes the refusal or names the member that does not fit.

  The failure's message is within 500 characters. Shown, a failure leads
  its message with the provider's name and the status, `openai (401): `, as
  Python's does. No control character is left in the message: each becomes
  a space, and a run of white space is one space. A character that shows
  nothing is dropped: a soft hyphen, a zero-width or a format character, a
  control of the direction of a text, a tag character.
- **Sizes.** Every size a server controls is bounded:
  - an answer read whole past 4 MiB fails the request;
  - a stream fails at a line past 1 MiB, at an event past 1 MiB, and past
    256 MiB in all;
  - a streamed answer fails past 1,024 calls, at a call whose arguments are
    past 4 MiB or whose id or name is past 1,024 bytes, and at a refusal
    past 4 MiB.

  Each is a final failure, after what was read before it. The text and the
  reasoning of a stream are handed on as they come and kept nowhere; under
  a response schema the contract keeps the answer's text to check it, within
  the stream's bound. Folding a streamed call costs in proportion to its
  bytes and to the number of its fragments, never to their product,
  whatever shape a server gives it: a fragment that brings no arguments
  costs nothing that grows with the call.
- **Where a request goes.** To the base URL of the configuration, OpenAI's
  own when it names none, followed by `/chat/completions` or `/models`. The
  path follows the base URL as Python's client writes it: after the one
  slash that ends the base URL, when one does (`/v1` and `/v1/` give
  `/v1/chat/completions`, `/v1//` gives `/v1//chat/completions`). A
  redirect is not followed: the request fails. A base URL is refused when
  the provider is built if it has a query or a fragment, if it holds white
  space, a control character or a backslash, if its scheme is not followed
  by `//` and a host, if its path holds a dot segment (`.` or `..`, written
  or percent-encoded), or if its user information is no UTF-8 once decoded
  (`%FF`): Python's client and this one would not send the same request for
  such a URL. The `Debug` forms show the origin the client dials, as the
  client's own URL parser reads it.
- **A base URL with user information** (`https://user:password@host/v1`) is
  sent with it as Basic credentials, `Authorization: Basic ...`, as Python's
  client sends it: they take the place of the key and of a configured
  `Authorization` header, neither of which is sent. The credentials are the
  user information once decoded, which must be UTF-8: a base URL whose user
  information is not is refused, as said above.
- **The environment.** The provider reads of it what the network of its
  host is, and nothing else. It is read once, when the provider is built: a
  variable that changes afterwards changes nothing for that provider.
  - **Proxies.** `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY`
    are read as Python's client reads them, in any case, the name that ends
    in lower case first: a request for an `http` endpoint is handed to its
    proxy, one for an `https` endpoint goes through a tunnel of its proxy,
    and a host `NO_PROXY` exempts is reached directly. A proxy's URL may
    carry credentials, which are sent to the proxy alone and appear in no
    failure. A SOCKS proxy is refused when the provider is built, and the
    system's own proxy settings, which Python's client takes on macOS and
    on Windows when no variable is set, are not read.
  - **Certificates.** A server's certificate is checked against the
    authorities of the file `SSL_CERT_FILE` names, else against those filed
    in the directories `SSL_CERT_DIR` names, else against the system's: its
    own verifier on macOS and on Windows, the store it keeps on its disk
    elsewhere. Python's client decides the same. One point goes beyond it:
    a system that keeps no authority at all, a container without a
    certificate store, is given the public roots compiled into the crate,
    which are never added to a store that holds an authority.
  - **Nothing else.** No key, no base URL, no organization, no project and
    no header is read. Python's client reads `OPENAI_BASE_URL`,
    `OPENAI_ORG_ID` and `OPENAI_PROJECT_ID`. A host that relied on
    `OPENAI_BASE_URL` to reach another server, and names none in its
    configuration, reaches OpenAI's own endpoint here, and sends it its
    key.

  `roomkit-providers-client`'s README says how each variable is read, where
  that differs from Python's client, and what was not verified: nothing
  here was run on macOS or on Windows. `examples/behind_a_proxy.rs` shows a
  host behind a proxy, with nothing of the proxy in its configuration.
- **Idle connections.** A request takes again a connection no request used
  for 5 seconds or less, as with Python's client, and 100 of them are kept
  for a host at most. One left with no request is closed between 5 and 10
  seconds after its last request. The connections in use are not capped: the
  HTTP client has no such bound, where Python's client holds 1,000 at most
  and makes the calls past them wait.
- **`User-Agent`.** Every request names its client,
  `roomkit-providers/` and the version of the crate, unless a default header
  of the configuration names a `User-Agent`, in any case, which replaces it.
- **No compressed answer.** The client sends no `accept-encoding`, whatever
  compression a host's build turns on in the HTTP client, so a server has no
  reason to compress. An answer compressed all the same is not decompressed:
  its body is no JSON, a failure.
- **Time.** `connect_timeout` bounds a connection. `timeout` bounds the wait
  for the head of an answer, from the moment its request is sent, then each
  wait between two reads of its body, not the whole answer: a stream may
  last longer, and one that stalls ends at `timeout`, a failure worth a
  retry. A timeout so long that the clock cannot count to its end is refused
  when the provider is built. A request is tried again only as `max_retries`
  allows, which is 0 unless the configuration says.

### What a failed request says

A request the server refuses fails with its status, in these words:

| The server answered | The failure says |
| --- | --- |
| `401` with `{"error": {"message": "Incorrect API key provided.", "type": "invalid_request_error", "code": "invalid_api_key"}}` | `the provider answered HTTP 401 (invalid_request_error, invalid_api_key): Incorrect API key provided.` |
| `404` with `{"detail": "Model m is not deployed here", "code": "model_not_found"}` | `the provider answered HTTP 404 (model_not_found): Model m is not deployed here` |
| `422` with `{"detail": [{"type": "missing", "loc": ["body", "messages"], "msg": "Field required", "input": {"model": "m"}}]}` | `the provider answered HTTP 422: body.messages: Field required` |
| `502` with an HTML page | `the provider answered HTTP 502: Bad Gateway` |
| `500` with an empty body | `the provider answered HTTP 500: Internal Server Error` |

The message, the type and the code are read in the body's `error` object,
or at its top level. A body that states no message and words its refusal
under `detail`, as a text, gives that text for one. A `detail` that is a
list, as a server that validates a request field by field writes it, gives
each entry's `msg` after the place its `loc` names, the steps of the place
joined by dots and the entries set apart by `; `. Nothing else of an entry
is repeated: its `input` holds what the request sent. `status_code` holds
the status, and `retryable` whether
it is one a new request may get past (408, 409, 429, 500 and above).
`context_overflow` is `true` when the server says the request is past the
model's context: by the code `context_length_exceeded`, or by its words,
read in the whole body, repeated in the failure or not.

## Where the provider differs from Python's

The provider does as `OpenAIAIProvider` of RoomKit's Python implementation on
295 of the 344 captured cases: 187 of the 211 of a whole answer, 99 of the
119 of a stream of events, the 4 of a stream of text, and 5 of the 10 of a
model listing. It differs on the others, each for a reason:

- **Redirects** are refused, where Python's SDK follows them, a `301`, a
  `302` or a `303` as a `GET` without the request's body. A request of a
  model's turn is not sent elsewhere than where the configuration says.
- **A base URL with a query or a fragment** is refused when the provider is
  built, where Python sends to a path written after the query. So are the
  forms listed above: Python sends one with a backslash to another host
  than this client would, one with no `//` or with white space to no host,
  and refuses one with a control character. Python sends one whose path
  holds a dot segment to that path as it is written
  (`/v1/%2e%2e/chat/completions`), where this client would take the segment
  out of the path; and for user information that is no UTF-8, Python sends
  credentials with replaced characters, where this client would send none
  or half of them.
- **An empty key** builds a provider that sends no `Authorization`, for a
  local server that asks for none, where Python's SDK refuses to build a
  client.
- **No time to the first token is measured** by the provider, where
  Python's reports one to its host. The Python package of this repository
  records it over any provider by one rule (`guide/python.md`): there a
  whole answer made of tool calls alone records none, where Python's
  provider records one for every whole answer.
- **`User-Agent`** is `roomkit-providers/` and the version of the crate,
  where Python's SDK names itself (`AsyncOpenAI/Python` and its version). A
  default header that names one replaces it, here as in Python.
- **An idle connection the client no longer takes** is closed by the client
  itself, between 5 and 10 seconds after its last request, where Python's
  client closes it when its pool is next asked for a connection.
- **The proxies and the certificate authorities** are those Python's client
  reads from the same environment, with these differences: a SOCKS proxy is
  refused when the provider is built, where Python's client takes one once
  `socksio` is installed; the system's proxy settings are not read, where
  Python's client takes them on macOS and on Windows when no proxy variable
  is set; a proxy without a host or with a port past 65,535 is refused when
  the provider is built, where Python's client fails at each request; an
  address of `NO_PROXY` is compared with the address the client dials, where
  Python compares it with the text of the base URL; and a system that keeps
  no certificate authority is given the public roots compiled in, where
  Python's client trusts nothing. A certificate the client refuses fails as
  `the provider's certificate is not trusted`, or says that it is for
  another host name or out of date, where Python says `Connection error.`;
  both type it as a connection that failed, worth a new request.
- **A count of tokens** is read when it is a JSON whole number, a float with
  no fraction included. Any other (a text, `true`, a fraction, a number below
  zero) stays unknown, whole or streamed, where Python converts some and
  fails on others, with an error of its own on a whole answer.
- **A failure's message** for a status is the provider's own sentence, as
  the table above shows, where Python's carries the whole body the server
  answered: of a `detail` that is a list, each entry's words and its place,
  where Python's holds the entries whole, with what the request sent under
  `input`. A body that is no JSON is not repeated, where Python's message
  is that body. A request that got no answer fails in this crate's words,
  where Python has the SDK's.
- **A context too long** is said by the failure itself, `context_overflow`,
  when the body says so by its words, where Python leaves it unsaid for a
  reader of its message, which holds the body.
- **An error object with no message**, answered in place of a completion, is
  written as JSON, where Python writes its `repr`.
- **Choices that are no list** fail with a `ProviderError`, final, whole as
  streamed, where Python's whole answer raises a raw `AttributeError`.
- **A 200 whose body is JSON and no completion** (`{"status": "ok"}`, `{}`,
  `{"choices": null}`) fails as no chat completion, final, where Python
  reads it as an answer that says nothing. A completion holds its choices as
  a list, an empty one included. Where such a body words why, under
  `message` or else under `detail`, the failure carries those words.
- **A stream that ends with neither a finish reason nor its end mark** fails
  after what came, as a stream whose connection is lost does, and hands no
  whole call, where Python reads it as complete and hands its call to run.
  A stream that ends with one of the two is complete, here as in Python.
- **A body that ends inside an event** is read as one that ended before
  that event, here as in Python, whose SDK reads nothing of such an event:
  it is complete if it had said why it stopped, and fails as said above if
  it had not, where Python reads it as complete. A body that ends inside a
  character is cut there too, where Python fails for good, in the words of
  its decoder.
- **A JSON error object that answers a stream request with a 200** fails as
  the status it names, retried or not by that status, as `generate` reads
  the same body. Python says the stream carried no event, a final failure
  that names no status.
- **An empty finish reason** (`"finish_reason": ""`) is none, here as in
  Python, and it is no stop. With its end mark such a stream is complete
  and has no reason, here as in Python. With no end mark it was cut, as
  said above: what came is handed on, then it fails, and a call it brought
  is never handed whole, where Python reads it as complete. So a server
  that writes an empty finish reason on every chunk keeps the detection of
  a cut. A whole answer keeps the empty text as its finish reason, as
  Python's does.
- **A byte order mark that starts a stream** is ignored, as the format of a
  stream of events says, and the first event is read, where Python's SDK
  reads the line the mark starts as no data.
- **The last event of a stream** that ends with no blank line after it is
  read when its data came whole, where Python's SDK drops it.
- **An event that holds no data** (a named event a server keeps a connection
  alive with) is no event, as the format says, where Python's SDK reads it
  as JSON and fails the stream.
- **A stream is bounded**, as said above, where Python's SDK reads a line or
  an event of any length, and its provider any number of calls.
- **A call's index** in a stream is a whole number; another fails the
  stream, where Python takes any value as the call's key.
- **Whether a call's arguments are whole**, which the fold of a stream's
  calls asks to tell whether a fragment on the same index starts another
  call, is asked of the JSON parser the contract reads with, and that
  parser's differences from Python's hold here. Arguments that hold a lone
  surrogate escape (`{"s": "\ud83d"}`), that nest 128 levels or more, or
  that are a whole number past what a float holds are whole to Python and
  not here: a fragment that names another tool after them starts another
  call in Python, and goes on with the same call here. The white space
  around arguments is read as Python reads it.
- **`list_models`** fails with a `ProviderError`, as a completion does, where
  Python's raises the openai SDK's error or a raw exception as it is: a
  status, a connection that failed, a body that is no page of models.
- **`supports_vision`** answers as Python's: what the catalogue says of the
  model; for a model it does not know, yes behind a base URL, and on OpenAI's
  own endpoint by how the model's name starts. The configuration's `vision`,
  which Python does not have, states it where the provider would not:
  `true` answers yes.
- **Whether the endpoint is OpenAI's own**, which decides the rules of its
  models, is Python's comparison of the base URL, and also what the client
  dials: both must say so.
- **The timeout** bounds the wait for the head of an answer from the moment
  its request is sent, connecting and sending included, then each wait
  between two reads of its body. Python's bounds each read and each write
  of the connection apart, and the wait for a connection of its pool.
  Neither bounds the whole answer.
- **`close()`** releases nothing: the connections close when the provider
  and its clones are dropped. Python's closes its HTTP client.
- **No transport is given to the provider.** Python's takes one, which its
  tests answer from; this one is tested against a local server.

A member of an answer written in another shape than the dialect's (a usage
that is a text, a delta that is no object) reads as absent. What Python
fails a stream on fails it here, in this crate's words: a chunk that is no
object, choices or calls that are no list, a text, a finish reason or a
call's arguments of the wrong type.

The rules of a try, which Python leaves to the openai SDK, are the SDK's: a
connection that failed, a timeout, an answer that says `x-should-retry`, or a
status of 408, 409, 429 or 500 and above is tried again, after the wait
`retry-after-ms` or `retry-after` asks, or, when they ask none above zero
that reads, after the backoff: 0.5 s doubling to 8 s, up to a quarter less.
An answer that asks a wait past two minutes is not tried again: the request
is given up with that answer's failure, as in Python's SDK. A whole answer
is read inside its try; a stream is tried until its head came. A
`retry-after` is read as the SDK reads it but for these, where "the
backoff" is a new try after the backoff's wait:

| The header says | Python's SDK | Here |
| --- | --- | --- |
| a number past what a float holds (`1e400`) | gives the request up: no wait is that long | no wait is taken from it: the backoff |
| digits with underscores (`1_0`) | 10 s | no number: the backoff |
| a date with no zone | read as UTC | no date: the backoff |
| a date with single-digit fields (`Thu, 1 Jan 2026 0:0:7 GMT`) | read | no date: the backoff |
| a date whose day is past its month (`32 Jan`) | the first day of the next month, too far to wait for: gives the request up | no date: the backoff |

It depends on RoomKit's Rust core for what a provider is (`roomkit-ai`), and
on `roomkit-providers-client`, the HTTP client every provider of this
repository is built on. It needs no Python.

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
- `vision` is this crate's: it states that the model takes images where the
  provider, which answers as Python's, would not say so. It is no part of
  the JSON form.
- `api`, `tool_namespace` and `token_source` are this crate's too, with the
  Responses API and the source of tokens: the JSON form holds none of them,
  and a configuration read from it has their defaults.
- `default_headers` are written sorted by name.
- Input is stricter: a value pydantic would convert (`"30"` for a number,
  `1024.0` for an integer, `1` for a boolean) and an integer outside 32 bits
  without a sign are refused.
- A member written twice in the JSON text is refused, where Python keeps the
  last, and an integer of `extra_body` outside 64 bits is read as a float:
  both come from the JSON parser.
- The same parser reads a request written as JSON, which is how one reaches
  the provider from Python. An integer outside 64 bits, in a tool's schema
  or in the arguments of a call a message replays, is read as a float and
  sent as one (`1180591620717411303424` goes as `1.1805916207174113e21`),
  where Python keeps its digits: a schema typed `integer` then no longer
  describes the value. A `temperature` that is no number, a `max_tokens`
  below zero or from 2**32 on, and a text that holds a lone surrogate are
  refused before the request, where Python sends what it can. A NaN among
  a call's arguments goes as `null`, where Python writes `NaN`.
- A base URL that Python's URL reader refuses (a bracket that holds no IPv6
  address, a character that normalizes to a delimiter) makes Python refuse
  the configuration. Here the configuration is a plain value: it is not
  OpenAI's endpoint, and nothing is refused.

The `Debug` form never prints the key, nor the base URL's user information,
path or query, nor a default header's value. Of the base URL it shows the
origin the HTTP client dials, read as the client reads it. The JSON form is
not for showing: like Python's, it masks the key alone, and writes a
default header's value and the credentials a base URL carries whole.

## Image generation

`OpenAiImage` is `OpenAI`'s image provider (RFC §25), RoomKit's Python
`OpenAIImageProvider`, on the contract's `roomkit_ai::image::ImageProvider`.
Its configuration, `OpenAiImageConfig`, is apart from the conversational one,
as Python's `OpenAIImageConfig` is: another endpoint, of models disjoint from
the conversational ones, two minutes to answer and five seconds to connect
unless given. Its catalogue, `catalog/images.json`, is Python's
(`scripts/generate_openai_catalogue.py`): each model with its image
capabilities.

- A generation is sent to `images/generations` as JSON; a request with
  reference images is an edit, sent to `images/edits` as multipart, the
  references as `image[]` and the mask after them, as Python's SDK writes
  them. The request's controls are merged over the configuration's, and the
  model's capabilities refuse what it cannot serve before anything is sent.
- `partial_images` asks a stream of previews, each told to the progress
  callback, then the final image.
- One try: a paid generation is never tried again on its own. Each outcome
  is an attempt, told to the callback as it moves. A failure once the
  request started is an `ImageGenerationError` that keeps the images and
  the usage it got: its attempt is unknown where the request was lost once
  sent, which the vendor may have run and billed; failed where it never
  left, or where the vendor answered with a status, a 500, 502 or 504 of
  which may still follow a run it billed (RFC §25.2): the host decides.
- A generation cancelled while its request is in flight (its future
  dropped once the attempt was told started) tells its callback the
  attempt unknown, as Python's provider does from its `CancelledError`.
  The callback is told in a task of the runtime the generation is dropped
  on, else, dropped off any, of the one it went in flight on, which the
  caller cannot await: a runtime that ends at once may end it first.
- An answer is read to 512 MiB, an event of a stream to 64 MiB: ten images
  of the largest size, one in an event. Python's client reads any length.
- The usage is split as Python splits it: the image share of the input
  apart from the text's, every output token an image's, no counter made up
  for one the answer does not report.

A failure's text is the provider's own words, as the conversational
provider's, a server's message cleaned of the key and the configured
headers' values: Python's repeats the server's whole body in Python's
`repr`. A multipart header that holds a control character is refused, as
Python's client refuses it, and a body's boundary is drawn at random.

### OpenRouter's images

`OpenRouterImage` is OpenRouter's image provider, RoomKit's Python
`OpenRouterImageProvider`, configured with `OpenRouterImageConfig`
(`OpenAIImageConfig`'s members, OpenRouter's endpoint and its attribution
headers, `site_url` as `HTTP-Referer` and `app_name` as `X-Title`). Its
catalogue, `catalog/openrouter_images.json`, is Python's.

- A request is OpenRouter's Image API's (`POST /api/v1/images`, the key as a
  bearer): the size as pixels, the configuration's controls, the references
  as `input_references`, inline ones written again as canonical data URIs.
- `n` images are `n` requests made at once, at most ten; the first failure
  stops the others, as Python's task group cancels them. Each image carries
  its own request's usage: the prompt's tokens as the text input, the
  completion's as the image's, and `cost`, what OpenRouter billed, in USD.
- One try. A refusal is a `ProviderError` in Python's words, OpenRouter's
  status then what it answered, tried again only where the vendor ran
  nothing (408, 409, 429, 503); a request lost in transport only where it
  never left. Its text is bounded to 500 characters, the provider's words
  with the server's, where Python repeats the server's first 500 after its
  own (`a_refusal_of_a_long_body`, named).
- The controls of `generate_with_options`, a mask and a progress callback
  are refused, as Python's provider refuses them: it answers `generate`
  alone.

It is held to 41 cases captured on RoomKit's Python provider
(`tests/fixtures/python_openrouter_image_cases.json`, written by
`scripts/generate_openrouter_image_cases.py`), 39 replayed as Python and 2
named.

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
fails. Every case is replayed against the provider, by the harness of
`roomkit-providers-client`: the requests it sends, the tries and the waits,
and what it gives are compared with Python's: the response, the events in
their order, the text, the models, and a failure with what the stream gave
before it. The fake serves a stream read by read,
each read the bytes of one recorded chunk, written and flushed before the
next; the replay requires that it wrote as many reads as the case records.
An id the provider mints for a call is compared by its shape. A case the
provider answers otherwise is named in the test with its reason, the
differences from Python its replay shows and what the provider does: the
replay must differ in those and in nothing else, and the provider must do
that and nothing else. A named case that starts to replay as Python fails
the test.

`tests/fixtures/python_image_cases.json` is written the same way by
`scripts/generate_openai_image_cases.py`, on RoomKit's Python
`OpenAIImageProvider`: a case holds what the image surface was asked (the
prompt, the size, the count, the references, the controls, the mask, and a
progress callback that records each attempt or fails), the requests sent, a
multipart body as its parts, and what Python gave: the images, or the error
with its attempts and its images, and every attempt the callback was told
of. The replay compares each, an attempt's id by the order it comes in, and
an attempt's error by its words where the case compares the message, else by
whether it has one. One case is named: a connection the fake refuses is one it accepted then closed,
which the client reads as lost once sent.

`tests/fixtures/named_cases.json` holds the named cases as data, written
from the test's own list and kept equal to it by a test: a replay outside
the crate, the Python package's, reads it. That replay drives a provider
through the crate's `replay` feature, which is no part of the provider a
host uses: a provider's requests are sent to a loopback address, its clock
is set, and its waits are kept instead of taken.

## License

MIT
