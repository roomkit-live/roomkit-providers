# roomkit-providers-anthropic

The Anthropic model provider of RoomKit Providers.

**Early development.** The crate holds today:

- `Anthropic`, a provider for Anthropic's Messages API and for the servers
  that speak it behind a base URL. It answers a request whole (`generate`),
  as a stream of events (`generate_structured_stream`) or of text
  (`generate_stream`), and lists the models its endpoint serves
  (`list_models`);
- `AnthropicConfig`, what the provider is configured with. It is the
  `AnthropicConfig` of RoomKit's Python implementation: the same names, the
  same defaults, and the JSON it gives with `model_dump(mode="json")`, read
  and written;
- the wire cases the provider is held to
  (`tests/fixtures/python_messages_cases.json`): what RoomKit's Python
  provider sends and reads on the Messages API and on the model listing,
  case by case. Every case is replayed against the provider through a local
  fake of the endpoint.

```rust
use roomkit_providers_anthropic::{Anthropic, AnthropicConfig};
use roomkit_providers_llm::{AiMessage, AiProvider, AiRequest, AiRole};

async fn ask() -> Result<String, roomkit_providers_llm::ProviderError> {
    let config = AnthropicConfig::new("placeholder-not-a-key", "claude-sonnet-4-6");
    let provider = Anthropic::new(config)?;
    let request = AiRequest {
        messages: vec![AiMessage::new(AiRole::User, "Hello")],
        ..AiRequest::default()
    };
    Ok(provider.generate(&request).await?.content)
}
```

`examples/local_messages.rs` asks once whole and once streamed, against a
stand-in server it starts on a local port, and checks what it read:

```sh
cargo run -p roomkit-providers-anthropic --example local_messages
```

The crate is the wire of Anthropic's API and nothing else: its
configuration, how a request is written, how the events of an answer are
read, how an error body is read, its catalogue and its recorded cases. The
HTTP client, the tries of a request, the bounds of what is read, the base
URLs refused when a provider is built and the one exit of failures are
`roomkit-providers-client`'s, which every provider of this repository is
built on.

## What a request carries

Every generation is asked as a stream, a whole answer too: `generate` folds
the events `generate_structured_stream` gives, as Python's does. A request
is a `POST <base URL>/v1/messages` whose body holds, in the order Python's
SDK writes them: `max_tokens`, `messages`, `model`, `output_config`,
`system`, `tools`, `thinking`, `stream` and `temperature`.

- **Messages.** A text is sent as it is, parts as Anthropic's content blocks:
  a text, an image (a remote URL as it is, an inline one as its media type
  and its canonical payload), a call, the result of a call, a block of
  reasoning with its signature, a redacted one as its opaque data. Reasoning
  without a signature is another vendor's and is not sent back. A tool's
  results and a `system` message of the history are a user turn.
- **A result that makes tools callable** carries their references alone, as
  the API requires; what the tool returned follows the message's results,
  named after its call and set apart in a `<tool_result>` block its text
  cannot close ([What is not Anthropic's wire](#what-is-not-anthropics-wire)).
- **Tools.** A tool held unseen (`defer_loading`) is declared so, unless
  every tool is. On Anthropic's own endpoint a name the vendor refuses fails
  before anything is sent.
- **Reasoning.** A turn that thinks asks for adaptive thinking or for its
  budget, as the configuration says, and sends no temperature. A turn that
  states thinking off is sent the block the catalogue declares for its
  model, and its effort goes as the nearest level the model takes.
- **Prompt caching.** Unless the configuration turns it off, four marks at
  most: the last tool shown, the system prompt, and the last two messages
  that hold a block to mark.
- **The key.** The configuration's, in `x-api-key`, or the key the request
  carries for itself under `api_key` in its metadata, a text that is not
  empty: it goes in that request's header alone, and no request leaves its
  key to the next. The model listing is asked with the configuration's. A
  request that has no key to carry, none of its own, none in the
  configuration and no credential among the extra headers, is refused before
  it is sent, as Python's SDK refuses it.
- **Headers.** `anthropic-version: 2023-06-01`, `accept: application/json`,
  and `content-type` on a request with a body; then the configuration's
  `extra_headers`, one of which replaces the provider's of the same name,
  the key included, as Python's SDK lets a default header do.

## Streams

`generate_structured_stream` gives the events of one generation as they
come: what the message's start tells of its input (a usage event, which a
generation cut before its end counts), the reasoning (each delta naming its
block, a block's signature as a delta of its own, a redacted block as its
opaque data), the text, the composition of each call (`ToolCallDelta`,
under the id the call ends with and the index of its content block), the
call itself when its block closes, then the done event, with the raw stop
reason, the usage, the model that answered and the category of a refusal.
`generate_stream` gives the text alone.

- **A stream is pulled.** Nothing is sent before the stream is first polled,
  and the answer is read only as its events are taken: a reader that stops
  taking them stops the reading.
- **Dropping the stream stops the request.** Its connection is closed,
  whatever was left of the answer; so does cancelling a `generate`. Nothing
  runs in the background.
- **A failure is the last item.** What was read before it was handed on
  first. A request is tried again, as `max_retries` allows, until the head of
  its answer came, never after: a stream that fails under way fails, and
  `retryable` says whether a new request may succeed.
- **An answer under a response schema** is checked before its done event,
  which its failure replaces. The text went out as it came: it is
  provisional until the done event.
- **The answer is read as a stream of events** whatever the server says it
  is. Lines end with a line feed, a carriage return, or both, and a byte
  order mark that starts the answer is ignored. An event is read by the
  `type` its data names. A page or a whole JSON body holds no event, which
  is a failure; a JSON error object in place of a stream fails as the status
  its type names.
- **A stream is complete once it said why it stopped, or sent the event that
  ends its message**, past which nothing is read. One that ends with neither
  was cut: what came is handed on, then it fails, a failure worth a retry,
  and a call it brought is never handed whole, to run.
- **A call left open** by a message that is complete (Anthropic closes no
  block a cut stops) is handed once the message is over, with the arguments
  that streamed, read for how the message stopped: partial when the response
  was cut over it, garbled when nothing cut it and its arguments do not
  read.

## The model listing

`list_models` asks `GET <base URL>/v1/models?limit=1000`, one page, with the
headers, the timeouts and the tries of any request. The page gives each
model's id and name: what the crate's catalogue knows of a model (its
context window, whether it takes images, its price) is filled from it, and
a model the catalogue does not know passes through. A body that is no page
of models with their ids is a failure, never an empty listing.
`available_models` is the catalogue, with no request.

## What the provider guarantees

- **Secrets.** No failure's text repeats a secret of the configuration or of
  the request that a server's answer holds: the key, the key a request
  brings for itself, the value of each extra header (and, where the value
  names a scheme such as `Bearer`, what follows it), and the user
  information of the base URL. Each is replaced by `[***]`, in every form
  the code that builds a failure writes it in. A secret shorter than 8
  characters is not searched for, and one a server itself sends back in
  another form cannot be recognised. The `Debug` forms print none of them,
  and the provider logs none.
- **What a failure repeats of an answer.** Only this, each on one line and
  bounded: of a request the server refuses, the type and the message of its
  error object; of an error object in place of an answer, or of an error a
  stream tells, its message, or the object itself, written as JSON, when it
  has none; of an answer a reader cannot read, the reader's words, which may
  quote the member it refuses; of an answer a response schema refuses, the
  contract's sentence. A body that is no JSON is never repeated. The
  failure's message is within 500 characters, without a control character
  or a character that shows nothing.
- **Sizes.** Every size a server controls is bounded: a model listing past
  4 MiB fails the request; a stream fails at a line or an event past 1 MiB
  and past 256 MiB in all; a message fails past 1,024 calls, past 1,024
  content blocks open at once, at a call whose arguments are past 4 MiB or
  whose id or name is past 1,024 bytes. Each is a final failure, after what
  was read before it. The text and the reasoning of a stream are handed on
  as they come and kept nowhere.
- **Where a request goes.** To the base URL of the configuration, Anthropic's
  own when it names none, followed by `/v1/messages` or `/v1/models`. A base
  URL that already ends with `/v1/messages`, in any case, is read without
  it. A redirect is not followed: the request fails. A base URL is refused
  when the provider is built if it has a query or a fragment, if it is
  empty or no `http` or `https` URL, if it holds white space, a control
  character or a backslash, if its path holds a dot segment, or if its user
  information is no UTF-8 once decoded: Python's client and this one would
  not send the same request for such a URL.
- **A base URL with user information** (`https://user:password@host`) is
  sent with it as Basic credentials, `Authorization: Basic ...`, beside the
  key, as Python's client sends it: they take the place of an `Authorization`
  among the extra headers.
- **The environment.** The provider reads nothing from it: no key, no base
  URL, no header. Python's SDK reads `ANTHROPIC_BASE_URL` and
  `ANTHROPIC_CUSTOM_HEADERS` when a client is given none. What the client
  the provider is built on reads of the environment, the proxies and the
  certificate authorities it names, is said in the README of
  `roomkit-providers-client`.
- **Time.** `connect_timeout` bounds a connection. `timeout` bounds the wait
  for the head of an answer, from the moment its request is sent, then each
  wait between two reads of its body, not the whole answer: a stream may
  last longer, and one that stalls ends at `timeout`, a failure worth a
  retry. A request is tried again only as `max_retries` allows, which is 0
  unless the configuration says.

### What a failed request says

A request the server refuses fails with its status, in these words:

| The server answered | The failure says |
| --- | --- |
| `401` with `{"type": "error", "error": {"type": "authentication_error", "message": "invalid x-api-key"}}` | `the provider answered HTTP 401 (authentication_error): invalid x-api-key` |
| `502` with an HTML page | `the provider answered HTTP 502: Bad Gateway` |
| `500` with an empty body | `the provider answered HTTP 500: Internal Server Error` |

The type and the message are read in the body's `error` object.
`status_code` holds the status, and `retryable` whether it is one a new
request may get past (408, 409, 429, 500 and above). An error a stream
tells, and an error object answered with a 200, carry the status their type
names (`overloaded_error` as 529, `rate_limit_error` as 429,
`invalid_request_error` as 400), or the one a numeric `code` names; one that
names none is final. `context_overflow` is `true` when the server says the
request is past the model's context by its words, read in the whole body:
Anthropic names no code for it.

## Where the provider differs from Python's

The provider does as `AnthropicAIProvider` of RoomKit's Python
implementation on 300 of the 364 captured cases: 208 of the 242 of a whole
answer, 80 of the 100 of a stream of events, 5 of the 6 of a stream of text,
and 7 of the 16 of a model listing. It differs on the others, each for a
reason:

- **Redirects** are refused, where Python's SDK follows them, a `301`, a
  `302` or a `303` as a `GET` without the request's body, and sends the key
  to the host a redirect names. The failure of a redirect carries its
  status, a redirect that names no location too, where Python's carries
  none.
- **A base URL with a query, and an empty one,** are refused when the
  provider is built. Python sends the first to a path written after the
  query, and reads the second as none: it asks Anthropic's own endpoint
  without the profile of its model and without the check of its tool names.
  So are the other forms listed above.
- **A count of tokens** is read when it is a JSON whole number, a float with
  no fraction included. Any other (a text, `true`, a fraction, a number below
  zero) stays unknown, where Python converts some and fails on others. A
  message whose start or end tells no usage, or whose end leaves out its
  output, is answered with what it told, where Python fails.
- **A failure's message** for a status is the provider's own sentence, as
  the table above shows, where Python's carries Python's `repr` of the whole
  body. A body that is no JSON is not repeated, where Python's message is
  that body, and a byte order mark before an error body is no part of it,
  where Python then takes the body for a text. A request that got no answer
  fails in this crate's words, where Python has the SDK's.
- **A context too long** is said by the failure itself, `context_overflow`,
  when the body says so by its words, where Python leaves it unsaid for a
  reader of its message, which holds the body.
- **A JSON error object that answers a request with a 200** fails as the
  status its type names, retried or not by that status, as the same error
  told as an event of a stream does. Python says the stream carried no
  event, a final failure that names no status.
- **A stream that ends with neither a stop reason nor the event that ends
  its message** fails after what came, as a stream whose connection is lost
  does, and hands no call, where Python reads it as complete, with no finish
  reason, and hands its call to run. A stream that ends with one of the two
  is complete, here as in Python.
- **An event is read by the `type` its data names**, where Python's SDK reads
  it by its `event:` line: here a stream whose events have no such line is
  read, one whose data names no type fails, and an error event whose data is
  no JSON fails in the reader's words, where Python's message is the event's
  text. The reader of events this crate is built on gives the data of an
  event, not its name.
- **A byte order mark that starts a stream** is ignored, as the format of a
  stream of events says, and the first event is read, where Python's SDK
  reads the line the mark starts as no event and fails on the next.
- **The last event of a stream** that ends with no blank line after it is
  read, where Python's SDK drops it, with the stop reason and the usage it
  holds.
- **A call whose arguments are blank or no JSON** is handed as the contract
  reads it: blank arguments as none, unreadable ones as a call that does not
  run. Python's SDK fails the whole stream on them.
- **A call is handed once.** One whose block names no id is handed under the
  one id the provider gave it, where Python hands it twice, under two ids;
  one left open under an id an earlier call of the message carried is handed
  when the message is over, where Python tells its composition and never
  hands it.
- **What cannot be folded fails the stream:** a message that starts twice, a
  content block that opens twice, a delta or a closing that names no open
  block, a text or a reasoning delta in a block of another kind, a call that
  names no tool, a redacted block that holds no data, an index that is no
  whole number. Python's SDK folds some of these without a word. An empty
  stop reason is none.
- **Nothing is read past the event that ends a message**, where Python reads
  the stream to its end.
- **A stream is bounded**, as said above, where Python's SDK reads a line or
  an event of any length, and its provider any number of calls.
- **A turn whose role the contract does not name** is refused when the
  request is read, before the provider sees it, where Python sends the role
  as it is written.
- **`list_models`** fails with a `ProviderError`, as a message does, where
  Python's raises the anthropic SDK's error or a raw exception as it is: a
  status, a connection that failed, a body that is no page of models. An
  error object in place of the page fails as the status its type names.
- **One client serves every key.** A request's key goes in that request's
  header, where Python keeps a separate SDK client, with its own connections,
  for each of the last eight keys.
- **The connections in use are not capped**: the HTTP client has no such
  bound, where Python's client holds 1,000 at most and makes the calls past
  them wait. A connection no request uses is kept 5 seconds, and 100 of
  them for a host at most, as Python's client keeps them.
- **No time to the first token is measured**, where Python's provider
  reports one to its host.
- **Whether the endpoint is Anthropic's own**, which decides the check of
  tool names and whether a tool may be held unseen, is Python's comparison
  of the base URL, and also what the client dials: both must say so.
- **The timeout** bounds the wait for the head of an answer from the moment
  its request is sent, connecting and sending included, then each wait
  between two reads of its body. Python's bounds each read and each write of
  the connection apart, and the wait for a connection of its pool. Neither
  bounds the whole answer.
- **`close()`** releases nothing: the connections close when the provider
  and its clones are dropped. Python's closes its HTTP clients.

The provider does as Python on the other questioned cases of the fixture: a
token bound of zero in the request is read as none; an `X-Api-Key` among the
extra headers replaces the configuration's key and a request's; a refused
request whose body is lost fails as a lost connection, worth a retry; a
stream that said why it stopped and ends without the event that ends its
message is complete; the arguments a call's block holds as it opens are not
read; and the one page of a listing that says more follow is the listing.

The rules of a try, which Python leaves to the anthropic SDK, are the SDK's:
a connection that failed, a timeout, an answer that says `x-should-retry`,
or a status of 408, 409, 429 or 500 and above is tried again, after the wait
`retry-after-ms` or `retry-after` asks, any wait above zero up to 4,294,967
seconds, or, when they ask none that reads, after the backoff: 0.5 s
doubling to 8 s, up to a quarter less. A stream is tried until the head of
its answer came. A `retry-after` written as digits with underscores, as a
date with no zone or with single-digit fields, or as a date whose day is
past its month, which the SDK reads, is no wait here: the backoff is taken.

## Where the configuration differs from Python's

- `timeout` and `connect_timeout` are a `Duration`, where Python holds
  seconds as a float. The JSON form writes seconds, as Python's; a timeout is
  kept to the nanosecond, and one below zero is refused.
- `use_adaptive_thinking` and `supports_custom_temperature` are
  `Option<bool>`: `None` is a switch left unset, which takes the profile of
  the model on Anthropic's own endpoint, as Python decides it when the
  configuration is built. The JSON form writes the switch decided, `true` or
  `false`, as Python does, and a member left out of what is read leaves it
  unset.
- `base_url` holds what it was given. Python's configuration holds it
  without the white space around it, the slashes that end it and a
  `/v1/messages` it ends with; here that form is what the JSON form writes
  and what the provider reads.
- `extra_headers` are written sorted by name, where Python keeps the order
  they were given in.
- Input is stricter: a value pydantic would convert (`"30"` for a number,
  `2048.0` for an integer, `1` for a boolean) and an integer outside 32 bits
  without a sign are refused. A member written twice in the JSON text is
  refused, where Python keeps the last.
- A base URL that Python's URL reader refuses (a bracket that holds no IPv6
  address, a character that normalizes to a delimiter) makes Python refuse
  the configuration. Here the configuration is a plain value: it is not
  Anthropic's endpoint, and nothing is refused until a provider is built.

The same JSON parser reads a request written as JSON, with the differences
the contract's documentation lists: an integer outside 64 bits is read as a
float, and a number JSON does not name is refused.

The `Debug` form never prints the key, nor the base URL's user information,
path or query, nor an extra header's value. Of the base URL it shows the
origin the HTTP client dials. The JSON form is not for showing: like
Python's, it masks the key alone.

## What is not Anthropic's wire

Where a result follows its tool references, the provider writes it as text,
with RoomKit's rule for text a model reads that neither the host nor the
provider wrote (RFC §6.4). The rule is the contract's, which every provider
that writes such a text reads it from (`roomkit-providers-llm`, held there to
what RoomKit's Python gives):

- `fence` sets the result apart in a `tool_result` block it cannot close: a
  closing or an opening tag of the block's name the text holds is
  neutralised, in any case and spacing, in the forms Unicode lists as
  confusable with its brackets, its slash and its letters, with an invisible
  or a combining character anywhere in it, and with its separator as any
  dash, underscore or space;
- `identifier` writes the tool's name outside any quote: letters, digits and
  `_ . @ + -`, anything else a dash.

## The wire cases

`tests/fixtures/python_messages_cases.json` is written by
`scripts/generate_anthropic_messages_cases.py`, which runs RoomKit's Python
`AnthropicAIProvider` against a fake of the endpoint, with no network. A case
holds the configuration, the request in the contract's JSON form, each HTTP
request Python sent, with the URL it went to, and the answer the fake gave,
and what Python returned: a response, the events or the text of a stream,
the models of a listing, or the error it raised. The fixture's `notes` say
how each part is written, and three members say how a replay takes a case:
`compare`, `sdk` and `questioned`.

The crate's tests read every case with `AnthropicConfig` and the contract's
types, and require every kind of case: a regenerated fixture that loses one
fails (`tests/python_messages_cases.rs`, on the shape checks of
`roomkit-providers-client`). Every case is replayed against the provider, by
the harness of `roomkit-providers-client`: the requests it sends, the tries
and the waits, and what it gives are compared with Python's: the response,
the events in their order, the text, the models, and a failure with what the
stream gave before it. The fake serves a stream read by read, each read the
bytes of one recorded chunk; the replay requires that it wrote as many reads
as the case records. An id the provider mints for a call is compared by its
shape. A case the provider answers otherwise is named in the test
(`src/replay/named.rs`) with its reason, the differences from Python its
replay shows and what the provider does: the replay must differ in those and
in nothing else, and the provider must do that and nothing else. A named
case that starts to replay as Python fails the test.

`tests/fixtures/named_cases.json` holds the named cases as data, written
from the test's own list and kept equal to it by a test: a replay outside
the crate reads it. Run that test with `NAMED_CASES_OUT` naming the file to
rewrite it. A replay outside the crate drives a provider through the crate's
`replay` feature, which is no part of the provider a host uses: a provider's
requests are sent to a loopback address, its clock is set, and its waits are
kept instead of taken.

`tests/fixtures/python_config_forms.json` holds the JSON forms of Python's
`AnthropicConfig`, the values it refuses and converts, and what it keeps of
a base URL; `catalog/models.json` is Python's catalogue.

## License

MIT
