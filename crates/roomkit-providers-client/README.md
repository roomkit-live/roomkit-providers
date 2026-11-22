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
proxies and the certificate authorities it reads from the environment, the
tries of a request and their waits, the bounds of what is read, the base URLs
refused when a provider is built, the one exit of failures, and, for the
provider's tests, the replay of its recorded cases.
`roomkit-providers-openai` is written on it: each section below names the
file of that crate that shows it in full.

It depends on RoomKit's Rust core for what a provider is asked and answers,
and on no Python. It is not for a host: a host uses a
provider.

## What it holds

| Module | What |
| --- | --- |
| crate root | `Client`, built from the `Settings` a provider states with its `Timeouts`, and `Client::exit`, the exit of its failures; `Call`, one request; `configured_header`, a header of a configuration, its value shown by no `Debug` form; `Pace`, the clock and the wait a test replaces; `ANSWER_LIMIT`; `USER_AGENT`, how a request names the client; `is_json`; the macro `tried_again!` |
| `endpoint` | `joined`, the URL of a path under a base URL, which refuses the base URLs Python's client and this one read otherwise; `has_user_information`; `is_vendor_endpoint` and `talks_to_vendor`, whether a base URL is the vendor's own endpoint; `shown_url`, what a `Debug` form may show of a URL |
| `retry` | `Rule` and `Past`: whether a failed try is tried again and after what wait, and where the rule of a vendor's SDK differs |
| `failure` | `Failure`, a failure inside a provider, with the failures every provider words alike; `own`, `told` and `Stated`, for those a provider words; `Exit`, the one way a failure leaves, which a client makes, and `Exit::unbuilt` for a provider that has none yet; `Secrets`; `carried`; `described_status`, `retryable_status`, `names_transient`, `past_mark`, `one_line`; `MESSAGE_LIMIT`, `NAME_LIMIT` |
| `stream` | `Streamed`, a streamed answer read event by event within `STREAM_LIMIT`; `leaving` and `text`, the streams a provider gives of its events; `Item` |
| `catalogue` | What every provider reads the same way of its models: `read`, the catalogue a crate embeds; `entry`, the entry of a model; `tagged` and `reasoning_floor`, what an entry's tags say; `listed`, the models the page of a listing names |
| `harness` (feature `harness`) | Test support: `fake`, a local fake of an endpoint or of a proxy, over plain HTTP or over TLS; `tls`, an authority a test makes for itself and the certificates it signs; `Fixture`, `Case`, `Replays`, `replayed_or_named`, the replay of recorded cases; `named`, the cases a provider answers otherwise than Python; `self_test`, the replay held to what it compares; `shape`, a fixture held to its shape, every member of its cases read; `exit`, the exit of a test that has no client. With it, `Client::pointed_at`, which sends a provider's requests to a fake, directly, `Client::set_pace`, which replaces the clock and the wait between two tries, and `Client::set_stream_limit`, which lowers the bound of a stream: a provider has none of the three |
| crate root (feature `replay`) | `Client::replaying`, `ReplayWaits` and `replayed_at`: the seam a replay outside a provider crate's tests drives a provider through |

## What the client does

- **TLS** is the configuration of `roomkit-providers-http`: ring and
  HTTP/1.1, a server's certificate checked against what the environment and
  the system name ([The environment](#the-environment)).
- **Proxies** are those the environment names
  ([The environment](#the-environment)).
- **No redirect is followed**, a proxy's own among them. A redirect is a
  failure, final, with its status (`Failure::redirect`).
- **No compressed answer** is asked for, whatever compression a host's build
  turns on in the HTTP client.
- **Timeouts** are the provider's `Timeouts`: `connect` to connect, `read` for
  the head of an answer and between two reads of its body, which also ends a
  stream that stalls. A timeout the clock cannot count a hundred years past
  is refused when the provider is built: `Timeouts::new` refuses it, and is
  the only way to make them.
- **`User-Agent`.** Every request names the client that sends it,
  `roomkit-providers/` and the version of the workspace (`USER_AGENT`): one
  that asks an answer whole, a stream or a listing. A header of the
  provider's that names a `User-Agent`, in any case, replaces it: an extra
  header of its configuration. The SDKs under RoomKit's Python providers
  name themselves there (`AsyncOpenAI/Python` and `AsyncAnthropic/Python`,
  each with its version), which the recorded cases leave out. The request
  that asks a proxy for a tunnel carries none, as with Python's client.
- **Idle connections.** A request takes again a connection no request used
  for 5 seconds or less, and never one that waited longer, as with the
  client of Python's providers. The client closes those it no longer takes
  when it looks at the connections it keeps, every 5 seconds: one left with
  no request stays open between 5 and 10 seconds, where Python's client
  closes it when its pool is next asked for a connection. 100 idle
  connections are kept for a host at most, as that client keeps them.
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

## The environment

A client reads the environment of its process once, in `Client::new`, when
its provider is built: a variable that changes afterwards changes nothing
for that client, and a provider built later reads it anew. It reads the
proxy variables and the variables that name certificate authorities, as the
client of RoomKit's Python providers reads them (httpx2, under the `openai`
and `anthropic` SDKs), so that a host that moves from a Python provider
keeps the network it had. It reads nothing else: no key, no base URL, no
header. The variables a vendor's SDK reads for those (`OPENAI_BASE_URL`,
`OPENAI_ORG_ID`, `OPENAI_PROJECT_ID`, `ANTHROPIC_BASE_URL`,
`ANTHROPIC_CUSTOM_HEADERS` and the like) are not read: a provider takes its
key, its endpoint and its headers from its configuration alone, and its
README says what that changes for a host.

### Proxies

`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY`, read as Python's
`urllib` and `httpx2` read them:

- **Names.** A variable is read in any case. A name that ends in lower case
  (`http_proxy`) wins over the others, and set to nothing it unsets them.
  Under `REQUEST_METHOD`, the mark of a CGI program, `HTTP_PROXY` is dropped
  unless its name ends in lower case, since a request's `Proxy` header would
  set it; `HTTPS_PROXY` and `ALL_PROXY` are kept.
- **Which proxy.** `HTTP_PROXY` serves the `http` endpoints, `HTTPS_PROXY`
  the `https` ones, `ALL_PROXY` the scheme that has no proxy of its own. A
  proxy written without a scheme is an `http` one. A request for an `http`
  endpoint is handed to the proxy whole; one for an `https` endpoint goes
  through a tunnel the proxy opens (`CONNECT`), and the endpoint's
  certificate is checked as without a proxy.
- **A proxy's credentials.** The user information of a proxy's URL is what
  stands before the last `@` of its authority: the user before its first
  colon, the password after it, each decoded once. A backslash, a second
  `@` or an encoded sign there is a part of the user or of the password
  (`http://CORP\jdoe:word@proxy.example:3128` gives the user `CORP\jdoe`),
  and never of where the proxy is. The two are sent to the proxy as Basic
  credentials (`Proxy-Authorization`), and never to the endpoint behind a
  tunnel. An authority ends at the first `/`, `?` or `#`: written there
  without being encoded, one of them ends the proxy's host, as with
  Python's client.
- **`NO_PROXY`** is a list between commas, each entry trimmed:
  - a name exempts itself and what is under it (`example.com` exempts
    `api.example.com`), and after a dot what is under it alone
    (`.example.com` does not exempt `example.com`);
  - an address exempts itself, and so does a range, which is read as the
    one address it is written with (`10.0.0.0/8` exempts `10.0.0.0` and
    nothing else);
  - `localhost` exempts itself alone;
  - an entry with a port exempts that port (`example.com:8443`), never the
    own port of an endpoint's scheme (`example.com:443` exempts no `https`
    endpoint);
  - `*` alone exempts everything; a star anywhere else stands for nothing
    (`*.example.com` exempts no host);
  - an entry written as a URL is a pattern of Python's client
    (`https://example.com`, `all://*.example.com`, `https://`).

Each of these is Python's reading, quirks included: the crate's tests hold
it to what Python's client decides of a table of 136 environments
(`tests/fixtures/python_proxy_cases.json`, written by
`scripts/generate_python_proxy_cases.py`), which records for each proxy the
user and the password Python's client gives it. A test sends each way of
writing them through a local proxy, which must receive the same. The proxy
matcher of the HTTP client itself is not used: it reads these variables
otherwise (the upper-case name first, a range as a range, no port, every
proxy dropped under `REQUEST_METHOD`), and sends directly where a proxy's
URL does not read. The HTTP client is handed each proxy as an address made
of its scheme, its host and its port, with its user and its password beside
it: no text of a variable is read as a URL a second time.

An environment Python's client refuses when it is built is refused when the
provider is built, as `the provider cannot be built: ...`, in words that
repeat nothing of the variable: a proxy that is neither `http` nor `https`,
a proxy or an entry of `NO_PROXY` that does not read as a URL (an address
that is none, a port that is no number, a control character, an entry past
ASCII or in Punycode).

Where the client differs from Python's:

- **A SOCKS proxy is refused.** Python's client takes `socks5` and
  `socks5h` when the `socksio` package is installed, which RoomKit does not
  install, and refuses them otherwise.
- **The system's proxy settings are not read.** On macOS and on Windows,
  Python's client takes the proxies of the system's settings when no proxy
  variable is set at all. This client reads the variables alone.
- **A proxy Python's client could not reach is refused when the provider is
  built**: one without a host, one whose port is past 65,535. So is a port
  written otherwise than in digits (`+3128`), which Python reads.
- **A proxy's name holds letters, digits, hyphens, dots and underscores.**
  One that holds another sign (a backslash, a percent sign, a quote) is
  refused when the provider is built. Python's client takes it, and asks
  the network for the name as it is written. The HTTP client under this
  one reads an address twice, with two readers that do not take such a
  sign alike: it would end the host at a backslash, decode one after a
  percent sign, and send around a proxy whose name holds a quote.
- **A proxy at a link-local IPv6 address with a zone is refused**
  (`http://[fe80::1%25eth0]:3128`), which Python's client takes. So is such
  an address among the entries of `NO_PROXY` (`fe80::1%eth0`).
- **A proxy's name that IDNA 2008 forbids and UTS 46 maps** (a full-width
  letter) is read here, as the name it maps to, where Python's client
  refuses it when it is built.
- **A proxy at an address of five numbers** (`http://1.2.3.4.5:80`), and one
  whose name holds a Punycode label that does not decode, are refused when
  the provider is built, where Python's client takes them as names.
- **An address of `NO_PROXY` is compared with the address the client
  dials**, where Python compares it with the text of the URL: `::1` exempts
  `http://[0:0:0:0:0:0:0:1]/` and `127.0.0.1` exempts `http://127.1/` here,
  and neither does with Python's client. An address written short is the
  address it stands for, in hexadecimal too: `127.1` exempts
  `http://127.1/`, as with Python's client, and `0x7f.1` or `127.1` exempt
  `http://127.0.0.1/` too, which Python's client sends through
  the proxy.

### Certificate authorities

A server's certificate, an endpoint's or that of a proxy reached over
`https`, is checked against:

1. the authorities of the file `SSL_CERT_FILE` names, and no other. A file
   that cannot be read, or that holds no certificate, is refused when the
   provider is built, as Python's client refuses it;
2. else the authorities filed in the directories `SSL_CERT_DIR` names, under
   the name OpenSSL looks one up by (the eight hexadecimal digits of its
   subject's hash, a dot and a number, as `openssl rehash` writes them), and
   no other. Directories that hold none leave no authority, and every
   server is then refused;
3. else the system: its own verifier on macOS and on Windows
   (`rustls-platform-verifier`), which is what Python's `truststore` asks
   there, and on Linux and the other Unix systems the store the system
   keeps on its disk, its bundle file and its directories of hashed names,
   as OpenSSL reads them.

A variable set to nothing is not set. Cryptography is ring's on every
system.

One point is this crate's own: **a system that keeps no authority at all**,
a container without a certificate store, is given the public roots compiled
in (`webpki-roots`), where Python's client would trust nothing. They are
never added to a store that holds an authority of its own, and never stand
in for what a variable names.

Nor do they stand in for **a store of the system that is there and does not
read**: a provider is not built on such a system, and the failure names the
store by its path and holds nothing of a file's content, as in `the provider
cannot be built: the system's certificate store at
/etc/ssl/certs/ca-certificates.crt cannot be read`. Python's client is
built on such a system all the same. On the systems whose store is read
from the disk:

| What `openssl-probe` finds | What the client does |
| --- | --- |
| No bundle and no directory, or ones that are no longer there | The compiled-in roots |
| A bundle that holds no block: empty, or words alone | The compiled-in roots |
| A directory with nothing filed under a hashed name | The compiled-in roots |
| A hashed name whose file is not there, a link to an authority that was removed | Passed over |
| A certificate that is no authority the client can check against, beside one that is | Left out, as a system's bundle may hold some |
| A bundle, a directory or a file under a hashed name that the process may not read | Refused |
| A directory where the bundle is expected, or under a hashed name; a file where a directory is expected | Refused |
| A block that is not written as a block is, in the bundle or under a hashed name | Refused |
| Blocks, none of which gives an authority: a block that holds no certificate, a key, a block of another kind | Refused |

Where the client differs from Python's:

- **A certificate filed in a directory under a hash that is not its own** is
  read, where OpenSSL does not find it.
- **A server's own certificate is not its authority.** A certificate a
  server signed itself and that says it is an authority, as `openssl req
  -x509` makes one, is trusted by Python's client when `SSL_CERT_FILE`
  names it, and refused here, as `the provider's certificate is not
  trusted`: a server's certificate is checked against an authority that
  signed it, and an authority is not taken for a server's certificate. A
  host gives the server a certificate issued by an authority of its own,
  and names that authority's certificate in the variable. A certificate a
  server signed itself that does not say it is an authority is trusted
  here.
- **Revocation.** On macOS and on Windows the system's verifier may check a
  certificate's revocation, which Python's client does not ask of it. On
  those two systems nothing here was run: see
  [What is not verified](#what-is-not-verified).

### What a host sees

| What happened | The failure | Tried again | `retryable` | `status_code` |
| --- | --- | --- | --- | --- |
| A proxy answered a request handed to it with a 407 | `the provider answered HTTP 407: Proxy Authentication Required`, or what the provider reads of its body | no | `false` | `407` |
| A proxy answered a request handed to it with a 502 | `the provider answered HTTP 502: Bad Gateway` | yes | `true` | `502` |
| A proxy refused a tunnel, whatever its status | `the connection to the provider failed` | yes | `true` | none |
| A proxy answered with a redirect | `the endpoint answered with a redirect (302), which is not followed` | no | `false` | `302` |
| A proxy is not there | `the connection to the provider failed` | yes | `true` | none |
| No authority the client trusts signed the server's certificate | `the provider's certificate is not trusted` | yes | `true` | none |
| The certificate is for another host name | `the provider's certificate is not for its host name` | yes | `true` | none |
| The certificate has expired, or is not valid yet | `the provider's certificate has expired, or is not valid yet` | yes | `true` | none |

Each is typed as RoomKit's Python provider types it: a proxy's answer to a
request handed to it cannot be told from the endpoint's, and a tunnel that
does not open or a certificate that is refused is a connection that failed,
which Python's client tries again and RoomKit calls worth a new request.
The words for a certificate are this client's: Python says `Connection
error.` for each.

A proxy's credentials are a secret of the process. `Client::secrets` holds
them beside the configuration's, so that a failure repeats none of them, in
any form a proxy may send them back in: the user and the password as the
proxy reads them and as the variable writes them, each and the two
together, and the header that carries them. The `Debug` form of a client
says how many proxies it has, and shows no URL.

### What is not verified

- **macOS and Windows.** The tests run on Linux. That the system's verifier
  is asked on macOS and on Windows, and what it answers, was not run; nor
  was the crate built for either.
- **A proxy reached over `https`.** The HTTP client speaks TLS to such a
  proxy, checked against the same authorities; no test has such a proxy.
- **The system's store on Linux** is read where `openssl-probe` finds it.
  How it is read is tested on directories a test makes, one test for each
  form a store may be found in; the three forms that rest on a file's mode
  are not run, and say so, where the tests run as root or as the root of a
  namespace of their own, which reads any file. That `openssl-probe` finds
  the store of a system was checked on no system but the one the tests ran
  on. One test does read the machine's own store: a client built with no
  variable set must refuse a server a test's authority signed, which holds
  wherever that store reads.

## Writing a provider on it

The provider holds its configuration, the URLs it asks, the headers every
request carries, a `Client` and the `Exit` that client makes
(`roomkit-providers-openai/src/provider.rs`, `src/provider/build.rs`).

```rust
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue};
use roomkit_providers_client::endpoint::joined;
use roomkit_providers_client::failure::{Stated, past_mark};
use roomkit_providers_client::retry::{Past, Rule};
use roomkit_providers_client::{Call, Client, Exit, Failure, Settings, Timeouts, tried_again};
use roomkit_ai::{AiContext, ProviderError};
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
        Self::built(key, base_url).map_err(|failure| Exit::unbuilt(LABEL, &failure))
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
        // The exit is the client's to make: it repeats nothing of the key,
        // nor of what the client alone knows.
        let no_headers: [&str; 0] = [];
        let exit = client.exit(LABEL, [key.to_owned()], no_headers, &messages);
        Ok(Self {
            messages,
            headers,
            client,
            exit: Arc::new(exit),
        })
    }

    /// One whole answer to `request`, written as `body`. Every failure
    /// leaves through the exit, with the request it answers.
    async fn ask(&self, request: &AiContext, body: &[u8]) -> Result<Value, ProviderError> {
        let call = Call {
            url: &self.messages,
            headers: &self.headers,
            body: Some(body),
            token: None,
        };
        let answered = self.client.send(call).await;
        let read = answered.and_then(|(body, _json)| {
            serde_json::from_slice(past_mark(&body)).map_err(|error| Failure::malformed(&error))
        });
        read.map_err(|failure| self.exit.leave_for(request, &failure))
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

- **Its label** (`Client::exit`, `Exit::unbuilt`), which each failure
  carries as its `provider`, and which `AiProvider::label` answers.
- **Its URLs**, each `endpoint::joined(base_url, path)`: the failure of a
  refused base URL is the client's, and the URL is not repeated in it.
  `endpoint::is_vendor_endpoint(base_url, &[OFFICIAL])` says whether a
  configuration calls the vendor's own endpoint, for its defaults;
  `endpoint::talks_to_vendor` says it of the URL the client dials, for the
  rules a provider applies there.
- **Its headers**, a `HeaderMap` the provider writes, for every request or
  for one: a `Call` carries the headers of its request, so a key a host lends
  one request goes in that request's headers alone. A request with a body
  says that the body is JSON unless a header already says what it is. A
  header of the configuration is made with `configured_header(name, value)`:
  a host may put a credential in a header of any name, so no `Debug` form
  shows the value of one, and that of a `Call` shows the names of its
  headers alone. Where `endpoint::has_user_information(url)`, the client
  sends that user information as Basic credentials, as Python's client
  does: they are the request's `Authorization`, and the client leaves out
  the one the call's headers hold, the key's or a configured one. A provider
  has nothing to leave out itself.
- **Its secrets**, to the exit its client makes,
  `Client::exit(label, keys, header_values, url)`: the keys, the value of
  each configured header, and the URL, for its user information. The client
  adds the credentials of the proxies its environment named, which a
  provider does not know: so the exit comes from the client, and a provider
  can make none that lacks them. The key a host lends one request is not
  among them: the exit reads it from the request a generation leaves with
  ([Failures](#failures)).
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
- **Its catalogue**, the `catalog/models.json` its crate embeds and the
  tags that file names. How it is read is here, in `catalogue`: `read`,
  `entry`, `tagged`, `reasoning_floor`, and `listed` for the page of a model
  listing, each model read as the provider says
  (`roomkit-providers-openai/src/models.rs`).

### A source of tokens

`Call::token` names a host's source of bearer tokens (`TokenSource`). The
client asks it before each try of the request and sends the token as the
try's `Authorization`, in place of one among the call's headers; an empty
token leaves the try without one. A source that gives no token ends the
request before anything is sent. The token is removed from what the client
hands on of the answer (a refusal's body, a whole answer, the data of each
event), so that no failure built from it repeats the token.

### Failures

Inside a provider a failure is a `Failure`, never the contract's
`ProviderError`: it keeps the provider's own words (`own`) apart from what a
server or a reader said (`told`, with the most characters the text carries of
it). `Exit::leave` makes the contract's error of it, the only place one is
made: there what was told is cleaned of the configuration's secrets, in every
form the code that builds a failure writes them in, put on one line without a
control character, and bounded; the whole text is within `MESSAGE_LIMIT`. A
secret shorter than 8 characters is not searched for.

**A generation leaves with its request.** A host may lend a key to one
request (`request.metadata["api_key"]`, the contract's `request_api_key`),
and no failure of that request may repeat it, whether the provider sends it
or not. So the exit of a generation takes the request, and reads the key
from it:

- `Exit::leave_for(request, &failure)` for an answer asked whole
  (`generate`);
- `stream::leaving(events, exit, request)` for a stream of events
  (`generate_structured_stream`), and `stream::text` over it for a stream of
  text (`generate_stream`).

The key is searched for in every form a secret of the configuration is. A
provider holds no exit of a request and adds no secret itself. `Exit::leave`
takes no request: it is for what no request brings a key to, the model
listing, which is asked with the configuration's key.

**A provider that cannot be built has no client**, and so no exit. Its
failure leaves through `Exit::unbuilt(label, &failure)`, which knows no
secret: the provider's own words leave as they are, and the mark stands
where a failure quotes a server or a reader. Nothing but such a failure
leaves that way.

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
use roomkit_ai::{
    AiContext, ProviderStream, StreamDone, StreamEvent, StreamTextDelta, TextStream,
};
use roomkit_providers_client::items::{Yielder, items};

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

/// What `generate_structured_stream` returns for `request`: every failure
/// through the exit, with the request, after the contract's check where
/// the provider has one (`checked_stream`).
fn structured<'a>(
    answer: Streamed,
    exit: &'a Exit,
    request: &'a AiContext,
) -> ProviderStream<'a> {
    leaving(events(answer), exit, request)
}

/// What `generate_stream` returns: the text of the same events.
fn as_text<'a>(answer: Streamed, exit: &'a Exit, request: &'a AiContext) -> TextStream<'a> {
    text(structured(answer, exit, request))
}
```

The stream is pulled: nothing is read before `next()` is asked, and dropping
the `Streamed` drops the request with its connection. A failure of the stream
does not leave where it is built: it is `carried` as the last item, so that
the contract's `checked_stream` may run over the events first, and
`stream::leaving` lets it out through the `Exit`, with any failure the check
built on the way. It takes the request the stream answers, whose own key no
failure of the stream repeats, one that comes after events included.
`stream::text` gives the text of a stream of events that already left.

An answer may end inside an event, with no blank line after it: `next()`
gives what came of that event, and `was_cut()` then says whether the answer
was cut inside it, which is when its data does not read as a JSON document.
A provider reads no such event: its answer is what it was before it,
complete if it had said how it ends, cut if it had not, and an answer that
carried no event if nothing came before it. An event whose data reads came
whole, and is read as any other. A wire that ends on a mark that is no JSON
(`[DONE]`) looks for its mark before it asks: the mark ends the answer with
or without a blank line after it.

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

With the feature, a test has what no provider has: `Client::set_pace`, which
replaces the clock and the wait between two tries; `Client::set_stream_limit`,
which lowers the bound of a stream, for a test that sends no answer of
256 MiB; and `harness::exit(label, keys)`, the exit of a test that has no
client to make one.

The fake also stands for what a host's network puts before an endpoint:

- **a proxy.** A request handed to a proxy is a request the fake answers,
  whose target is the whole URL; `Reply::Tunnel(address)` answers a
  `CONNECT` and carries the bytes to the fake at `address`;
- **TLS.** `harness::tls::Authority::new()` makes an authority when the test
  runs, whose key is kept nowhere. `Fake::serving_tls(replies,
  authority.serving(&["llm.example"]))` is a server whose certificate it
  signed for those names, and `authority.pem()` is what a file
  `SSL_CERT_FILE` names holds. `Fake::turned_away` returns once a client
  left the greeting, as one that refuses the certificate does.

A provider reads its environment when it is built, and a test cannot set a
variable of its own process, since the crates forbid unsafe code: a test of
the environment runs a child in the environment it gives it
(`tests/environment.rs`, and `roomkit-providers-openai/examples/
behind_a_proxy.rs`, which `make examples` runs). `scripts/no-network.sh`,
which runs the tests, drops the proxy and certificate variables of the
machine, so that no other test depends on them.

A provider a test points at a fake goes there directly, whatever proxy its
environment names: `Client::pointed_at` moves its URLs to the fake, a
loopback address, and has the client go through no proxy from then on, its
clones included. A developer's proxy is then handed no request of a test,
run with the script or without it. It is still read when the provider is
built: one that does not read fails that test.

The recorded cases are a fixture written by a script on RoomKit's Python
provider, in the form `roomkit-providers-openai/tests/fixtures/
python_chat_cases.json` has: for each case a configuration, a request, the
HTTP exchanges Python had and what Python returned. The provider's tests
replay it (`roomkit-providers-openai/src/replay/`):

1. **`impl harness::Replays for <Provider>`**, in the crate's tests:
   `KEY_HEADER`, the header that carries its key, in lower case; `built`,
   the provider of a case's configuration; `client`, the `Client` it is built
   on, whose clock the replay sets and whose waits it keeps; `point_at`, which
   sends its requests to the fake (`Client::pointed_at` with its URLs).
   `key_refused`, the 401 its endpoint refuses a key with, is written for it:
   an error object that says why under `error`. A provider whose wire words a
   refusal otherwise writes its own.
2. **The named cases**, a `harness::named::Named` of three lists of `Entry`:
   `questioned_otherwise`, `only_python` and `by_its_own_rule`. An entry is a
   case's name, the reason the provider answers it otherwise than Python, the
   differences its replay then shows, each by the words it starts with, and
   what the provider does `Instead` (`Does::Unbuilt`, `Fails`, `Answers`,
   `Unread`, `AsksWithoutAKey`). No case is skipped: a case is replayed as
   Python, or it is named. `Does::Unread` is a case whose request the
   contract, stricter than Python's on what a request holds, does not read:
   nothing is asked.
3. **One test for each call**, which states its counts:
   `harness::replayed_or_named::<Provider>(&fixture, &NAMED, "generate")`
   gives how many cases replayed as Python and how many are named and do as
   their entry states, and fails for a case that is neither.

   The same call holds the provider to the key of one request, with nothing
   to write: each case of a generation is asked once more, its request
   lending a key, by a fake that refuses it with a 401 whose message repeats
   that key (`key_refused`). The failure must say what the 401 says without
   the key, in its message and in its `Debug` form. A failure that says
   nothing of the 401 fails too, since it would show nothing; a case whose
   request is never sent is not counted, and a call none of whose cases
   shows the key kept out fails. It lives there, and is no member a provider
   must write, because every provider's replay already goes through it, once
   for each of the three generations: a provider cannot replay its fixture
   and leave the check out.
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
6. **The shape of the fixture** (`harness::shape`), in a test of the crate's
   `tests/`: `shape::Fixture::read` reads every member of every case,
   strictly, and each check holds the fixture to what a replay rests on: the
   requests and the results read as the contract's types and write back
   unchanged, every failure says what a replay reproduces of it and whose
   words its message holds, the questioned cases are the ones the test
   lists, each request went where its case expects, each chunk is the bytes
   of one read, and the fixture shows every kind of case. The provider
   states what is its own in a `shape::Shape` (its label, its SDK's classes
   and sentences, the members of a case that are its own), and checks
   beside them its configuration, its catalogue and its URLs
   (`roomkit-providers-openai/tests/python_chat_cases.rs`).

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
puts it back; what a request carries there that is no key (an empty text, a
number) is written as it is in both. Any other member of a case is the provider's own, which
`Case::member` gives as it is: what Python's provider read of its model's
catalogue entry, a capability it answers.

One header is compared otherwise than the fixture records it, once, in the
harness: the `User-Agent`. A fixture leaves out the one Python's SDK writes,
so a case records none unless its configuration sets one. The replay
requires that one where it is recorded, as any header, and on every other
request exactly the client's own (`USER_AGENT`): a provider that sends none,
or another, differs from its case.

## The replay seam

A replay outside a provider crate's tests, a binding's, drives a provider
through the `replay` feature, which the provider crate forwards as a feature
of its own: `replayed_at(urls, fake)` sends the provider's URLs to a fake on
a loopback address, and refuses any other; `Client::replaying(clock)` reads a
`retry-after` date against the clock given and keeps the waits instead of
taking them, in the `ReplayWaits` it returns, and has the client go through
no proxy of the environment from then on: a replay sends nothing off the
machine (`OpenAi::replaying` in `roomkit-providers-openai/src/provider.rs`).

## What it does not hold

- The contract: the request, the response, the events and the failure a host
  sees are RoomKit's core's (`roomkit-ai`).
- TLS and the reader of server-sent events: they are
  `roomkit-providers-http`'s, which depends on no HTTP client.
- Anything of a vendor's wire.
