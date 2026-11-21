# roomkit-providers-client

The HTTP client a model provider of this repository is built on: what every
provider that speaks HTTP does the same way, written once.

A provider crate holds its wire and nothing else:

- its configuration;
- how it writes a request: its paths, its headers, its body;
- how it reads an answer, and the events of a stream;
- how it reads an error body, and the failures it words itself;
- its catalogue;
- its recorded cases, with the list of those it answers otherwise than
  Python.

Everything else comes from this crate: the HTTP client and its timeouts, the
tries of a request and their waits, the bounds of what is read, the base URLs
refused when a provider is built, the one exit of failures, and, for the
provider's tests, the replay of its recorded cases.
`roomkit-providers-openai` is written on it: each section below names the
file of that crate that shows it in full.

It depends on neither Python nor RoomKit. It is not for a host: a host uses a
provider.

## What it holds

| Module | What |
| --- | --- |
| crate root | `Client`, built from the `Settings` a provider states with its `Timeouts`; `Call`, one request; `Pace`, the clock and the wait a test replaces; `ANSWER_LIMIT`; `is_json`; the macro `tried_again!` |
| `endpoint` | `joined`, the URL of a path under a base URL, which refuses the base URLs Python's client and this one read otherwise; `has_user_information`; `is_vendor_endpoint` and `talks_to_vendor`, whether a base URL is the vendor's own endpoint; `shown_url`, what a `Debug` form may show of a URL |
| `retry` | `Rule` and `Past`: whether a failed try is tried again and after what wait, and where the rule of a vendor's SDK differs |
| `failure` | `Failure`, a failure inside a provider, with the failures every provider words alike; `own`, `told` and `Stated`, for those a provider words; `Exit` and `Secrets`, the one way a failure leaves; `carried`; `described_status`, `retryable_status`, `names_transient`, `past_mark`, `one_line`; `MESSAGE_LIMIT`, `NAME_LIMIT` |
| `stream` | `Streamed`, a streamed answer read event by event within `STREAM_LIMIT`; `leaving` and `text`, the two streams a provider gives of its events; `Item` |
| `harness` (feature `harness`) | Test support: `fake`, a local fake of an endpoint; `Fixture`, `Case`, `Replays`, `replayed_or_named`, the replay of recorded cases; `named`, the cases a provider answers otherwise than Python; `self_test`, the replay held to what it compares |
| crate root (feature `replay`) | `Client::replaying`, `ReplayWaits` and `replayed_at`: the seam a replay outside a provider crate's tests drives a provider through |

## What the client does

- **TLS** is the configuration of `roomkit-providers-http`: ring, the public
  web PKI roots, HTTP/1.1.
- **No redirect is followed.** A redirect is a failure, final, with its
  status (`Failure::redirect`).
- **No proxy** is read from the system or its environment, and **no
  compressed answer** is asked for, whatever compression a host's build turns
  on in the HTTP client.
- **Timeouts** are the provider's `Timeouts`: `connect` to connect, `read` for
  the head of an answer and between two reads of its body, which also ends a
  stream that stalls. A timeout the clock cannot count a hundred years past
  is refused when the provider is built.
- **Idle connections**: one that no request uses is kept 5 seconds, 100 of
  them for a host, as the client of Python's providers keeps them.
- **Tries.** A request that fails is tried again up to `max_retries` times: a
  connection lost, an answer that did not come in time, and an answer its
  `retry::Rule` says is worth another try (`x-should-retry`, else a 408, a
  409, a 429 or a server error), after the wait the answer asks
  (`retry-after-ms`, `retry-after` as a number or as an HTTP date) or a
  backoff of 0.5 s doubling to 8 s, up to a quarter less. A whole answer is
  read inside its try; a stream is tried until the head of its answer came,
  never after. A request that could not be built is final.
- **Bounds.** An answer read whole past 4 MiB (`ANSWER_LIMIT`) fails the
  request; an error body past it keeps its status. A stream fails at a line
  or an event past 1 MiB and past 256 MiB in all (`stream::STREAM_LIMIT`),
  after what was read before is handed on.

## Writing a provider on it

The provider holds its configuration, the URLs it asks, the headers every
request carries, a `Client` and an `Exit`
(`roomkit-providers-openai/src/provider.rs`, `src/provider/build.rs`).

```rust
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue};
use roomkit_providers_client::endpoint::joined;
use roomkit_providers_client::failure::{Stated, past_mark};
use roomkit_providers_client::retry::{Past, Rule};
use roomkit_providers_client::{
    Call, Client, Exit, Failure, Secrets, Settings, Timeouts, tried_again,
};
use roomkit_providers_llm::ProviderError;
use serde_json::Value;

/// The label the provider's failures carry.
const LABEL: &str = "vendor";

/// Where the retry rule of the vendor's SDK stands.
const RETRY: Rule = Rule {
    longest_asked: 120.0,
    past_it: Past::Final,
};

/// How the vendor's error body reads: its type and its message, under
/// `error`. The words of the failure are the client's.
fn refused(status: u16, body: &[u8]) -> Failure {
    let parsed: Option<Value> = serde_json::from_slice(past_mark(body)).ok();
    let said = |name: &str| {
        let said = parsed.as_ref()?.get("error")?.get(name)?.as_str()?;
        Some(said.to_owned())
    };
    let stated = Stated {
        kind: said("type"),
        code: None,
        message: said("message"),
        overflow: false,
    };
    Failure::refused(status, body, stated)
}

struct Vendor {
    messages: reqwest::Url,
    headers: HeaderMap,
    client: Client,
    exit: Arc<Exit>,
}

impl Vendor {
    fn new(key: &str, base_url: &str) -> Result<Self, ProviderError> {
        // A provider that cannot be built says why in its own words.
        let nothing_to_hide = Exit::new(LABEL, Secrets::default());
        Self::built(key, base_url).map_err(|failure| nothing_to_hide.leave(&failure))
    }

    fn built(key: &str, base_url: &str) -> Result<Self, Failure> {
        let messages = joined(base_url, "/v1/messages")?;
        let no_header = |_| Failure::unbuilt("its key cannot go in a header");
        let mut value = HeaderValue::from_str(key).map_err(no_header)?;
        value.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", value);
        let client = Client::new(Settings {
            timeouts: Timeouts::new(Duration::from_secs(60), Duration::from_secs(5))?,
            max_retries: 2,
            retry: RETRY,
            refused,
            tried_again: tried_again!("roomkit::providers::vendor"),
        })?;
        let no_headers: [&str; 0] = [];
        let secrets = client.secrets([key.to_owned()], no_headers, &messages);
        Ok(Self {
            messages,
            headers,
            client,
            exit: Arc::new(Exit::new(LABEL, secrets)),
        })
    }

    /// One whole answer. Every failure leaves through the exit.
    async fn ask(&self, body: &[u8]) -> Result<Value, ProviderError> {
        let call = Call {
            url: &self.messages,
            headers: &self.headers,
            body: Some(body),
        };
        let answered = self.client.send(call).await;
        let read = answered.and_then(|(body, _json)| {
            serde_json::from_slice(past_mark(&body)).map_err(|error| Failure::malformed(&error))
        });
        read.map_err(|failure| self.exit.leave(&failure))
    }
}

let refused = Vendor::new("placeholder-not-a-key", "https://llm.example/v1?version=1");
assert_eq!(
    refused.err().map(|error| error.message).as_deref(),
    Some(
        "the provider cannot be built: its base URL has a query or a fragment, which the \
         path of the API cannot follow"
    )
);
```

What the provider supplies:

- **Its label** (`Exit::new`), which each failure carries as its `provider`,
  and which `AiProvider::label` answers.
- **Its URLs**, each `endpoint::joined(base_url, path)`: the failure of a
  refused base URL is the client's, and the URL is not repeated in it.
  `endpoint::is_vendor_endpoint(base_url, &[OFFICIAL])` says whether a
  configuration calls the vendor's own endpoint, for its defaults;
  `endpoint::talks_to_vendor` says it of the URL the client dials, for the
  rules a provider applies there.
- **Its headers**, a `HeaderMap` the provider writes, for every request or
  for one: a `Call` carries the headers of its request, so a key a host lends
  one request goes in that request's headers alone. A request with a body
  says that the body is JSON unless a header already says what it is. Where
  `endpoint::has_user_information(url)`, the client sends that user
  information as Basic credentials, which take the place of an
  `Authorization` the provider would write.
- **Its secrets**, `Client::secrets(keys, header_values, url)`: the keys, the
  value of each configured header, and the user information of the URL.
  `Exit::with_secrets` gives the exit of one request that brings a key of its
  own.
- **Its retry parameters**, a `retry::Rule`: the longest wait a server may
  ask for and what a longer one does. Python's openai SDK takes two minutes
  and gives the request up past them (`longest_asked: 120.0`, `Past::Final`);
  Python's anthropic SDK takes any wait up to 4,294,967 seconds
  (`longest_asked: 4_294_967.0`, `Past::Longest`). The count of tries is the
  configuration's `max_retries`.
- **How it reads an error body**, `Settings::refused`: the failure of a
  status that is neither a success nor a redirect. It reads what the body
  states into a `failure::Stated`, which `Failure::refused` words as `the
  provider answered HTTP 401 (type, code): message`, bounded, the status's
  reason where no message is stated, and an overflow of the context read from
  the whole body; or it words the failure itself with `Failure::said`
  (`roomkit-providers-openai/src/failure.rs`).
- **Its log target**, `tried_again!("roomkit::providers::<vendor>")`: the
  line a provider logs when a request is tried again.

### Failures

Inside a provider a failure is a `Failure`, never the contract's
`ProviderError`: it keeps the provider's own words (`own`) apart from what a
server or a reader said (`told`, with the most characters the text carries of
it). `Exit::leave` makes the contract's error of it, the only place one is
made: there what was told is cleaned of the configuration's secrets, in every
form the code that builds a failure writes them in, put on one line without a
control character, and bounded; the whole text is within `MESSAGE_LIMIT`. A
secret shorter than 8 characters is not searched for.

The failures every provider words alike are built here: `Failure::unbuilt`,
`before_request`, `unwritten`, `transport`, `redirect`, `refused`,
`malformed`, `unreadable`, `unreadable_value`, `error_object` (Python's
`error_body_failure`), `stream_error` (an error inside a stream, with the
words of the vendor's SDK for one that brings none), `no_event` and
`stream_unread`. A failure the contract built (`ProviderError`, of a request
it refuses or of an answer it checked) becomes a `Failure` with `From`, and
is cleaned whole when it leaves.

### Streams

`Client::stream(call)` sends the request until the head of an answer comes,
and gives a `Streamed`: `next()` gives the data of each event in order, as
the bytes arrive, then `None`. A provider reads its own wire from that data,
hands each event on before it asks for the next, and decides when the answer
is complete (`roomkit-providers-openai/src/stream.rs`):

```rust
use roomkit_providers_client::failure::carried;
use roomkit_providers_client::stream::{Item, Streamed, leaving, text};
use roomkit_providers_client::{Exit, Failure};
use roomkit_providers_llm::{
    ProviderStream, StreamDone, StreamEvent, StreamTextDelta, TextStream, Yielder, items,
};

/// Reads `answer`, handing its events to `out` as they come.
async fn read(mut answer: Streamed, out: &Yielder<Item>) -> Result<(), Failure> {
    let mut events = 0_usize;
    while let Some(data) = answer.next().await? {
        // The provider's own reading of one event's data.
        let delta = StreamTextDelta { text: data };
        out.give(Ok(StreamEvent::TextDelta(delta))).await;
        events += 1;
    }
    if events == 0 {
        // A JSON document in place of a stream, when the server said so.
        let error = answer.unstreamed();
        let error = error.as_ref().and_then(|answer| answer.get("error"));
        return Err(error.map_or_else(Failure::no_event, Failure::error_object));
    }
    out.give(Ok(StreamEvent::Done(StreamDone::default()))).await;
    Ok(())
}

/// The events of `answer`: a failure is the last of them, carried until it
/// leaves.
fn events<'a>(answer: Streamed) -> ProviderStream<'a> {
    Box::pin(items(move |out: Yielder<Item>| async move {
        if let Err(failure) = read(answer, &out).await {
            out.give(Err(carried(failure))).await;
        }
    }))
}

/// What `generate_structured_stream` returns: every failure through the
/// exit, after the contract's check where the provider has one
/// (`checked_stream`).
fn structured<'a>(answer: Streamed, exit: &'a Exit) -> ProviderStream<'a> {
    leaving(events(answer), exit)
}

/// What `generate_stream` returns: the text of the same events.
fn as_text<'a>(answer: Streamed, exit: &'a Exit) -> TextStream<'a> {
    text(structured(answer, exit))
}
```

The stream is pulled: nothing is read before `next()` is asked, and dropping
the `Streamed` drops the request with its connection. A failure of the stream
does not leave where it is built: it is `carried` as the last item, so that
the contract's `checked_stream` may run over the events first, and
`stream::leaving` lets it out through the `Exit`, with any failure the check
built on the way. `stream::text` gives the text of a stream of events.

## Testing a provider with the harness

The provider crate enables the harness for its tests alone:

```toml
[dependencies]
roomkit-providers-client = { workspace = true }

[dev-dependencies]
roomkit-providers-client = { workspace = true, features = ["harness"] }
```

No test reaches a service: each sends to `harness::fake::Fake`, a local
server that answers each connection with the next `Reply` of a script
(`Reply::json`, `Reply::stream`, `Reply::Refused`, `Reply::Silent`) and keeps
what it was sent (`Fake::seen`). No test waits on a timeout: it waits on what
the fake observed (`Fake::silent`, `Fake::closed`), then lets the time pass
at once (`harness::let_pass`). `harness::drained` reads a stream to its end.

The recorded cases are a fixture written by a script on RoomKit's Python
provider, in the form `roomkit-providers-openai/tests/fixtures/
python_chat_cases.json` has: for each case a configuration, a request, the
HTTP exchanges Python had and what Python returned. The provider's tests
replay it (`roomkit-providers-openai/src/replay/`):

1. **`impl harness::Replays for <Provider>`**, in the crate's tests:
   `KEY_HEADER`, the header that carries its key, in lower case; `built`,
   the provider of a case's configuration; `client`, the `Client` it is built
   on, whose clock the replay sets and whose waits it keeps; `point_at`, which
   sends its requests to the fake (`endpoint::moved_to` on each of its URLs).
2. **The named cases**, a `harness::named::Named` of three lists of `Entry`:
   `questioned_otherwise`, `only_python` and `by_its_own_rule`. An entry is a
   case's name, the reason the provider answers it otherwise than Python, the
   differences its replay then shows, each by the words it starts with, and
   what the provider does `Instead` (`Does::Unbuilt`, `Fails`, `Answers`,
   `AsksWithoutAKey`). No case is skipped: a case is replayed as Python, or it
   is named.
3. **One test for each call**, which states its counts:
   `harness::replayed_or_named::<Provider>(&fixture, &NAMED, "generate")`
   gives how many cases replayed as Python and how many are named and do as
   their entry states, and fails for a case that is neither.
4. **Two tests of the named cases**: `Named::set_apart(&fixture)`, which
   fails for an entry in the wrong list, and one that holds a data file equal
   to `Named::as_data(notes)`, for a replay outside the crate, a binding's.
5. **The self-test** (`harness::self_test`): the replay held to what it says
   it compares. `refuses`, `refuses_a_stream_served_in_one_piece` and
   `held_to_its_entry` replay a case, then make one thing of what was
   replayed wrong and require the replay to say so. The lists of what is made
   wrong whatever the wire are there (`REQUEST`, `LISTING`, `WAITS`,
   `ASKED_WAIT`, `ANSWER`, `FAILURE`, `SCHEMA`, `STREAM`, `CUT`, `TEXT`,
   `MODELS`, `SERVED_AS_A_STREAM`, `NAMED_ANSWER`, `NAMED_STREAM`,
   `NAMED_FAILURE`, `NAMED_UNBUILT`); the provider names the cases of its
   fixture each is run on, and adds the wrongs of its own wire: another path,
   another key, a header it does not send.

What a fixture must hold for the harness to read it: `clock`, the instant
Python's clock was stopped at; and for each case `name`, `call` (`generate`,
`generate_structured_stream`, `generate_stream` or `list_models`), `config`
(with the key under `api_key`), `request`, `exchanges` (each a `request`
with its `method`, `url`, `headers`, `timeouts` and `body`, a `response`, and
the `wait` taken before the next try), `expected`, and, where they apply,
`supports_vision`, `compare`, `questioned`, `minted_call_ids`,
and `request_api_key`. A recorded header names the key it carried as
`<config key>` or `<request key>`; the key a request carries for itself is
written masked in `request` and plain under `request_api_key`, and the replay
puts it back. Any other member of a case is the provider's own, which
`Case::member` gives as it is: what Python's provider read of its model's
catalogue entry, a capability it answers.

## The replay seam

A replay outside a provider crate's tests, a binding's, drives a provider
through the `replay` feature, which the provider crate forwards as a feature
of its own: `replayed_at(urls, fake)` sends the provider's URLs to a fake on
a loopback address, and refuses any other; `Client::replaying(clock)` reads a
`retry-after` date against the clock given and keeps the waits instead of
taking them, in the `ReplayWaits` it returns
(`OpenAi::replaying` in `roomkit-providers-openai/src/provider.rs`).

## What it does not hold

- The contract: the request, the response, the events and the failure a host
  sees are `roomkit-providers-llm`'s.
- TLS and the reader of server-sent events: they are
  `roomkit-providers-http`'s, which depends on no HTTP client.
- Anything of a vendor's wire.
