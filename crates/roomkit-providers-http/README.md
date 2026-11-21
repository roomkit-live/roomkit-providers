# roomkit-providers-http

Outbound URL validation for applications, and the HTTP support the model
providers of this repository share.

`validate_public_url` checks that a URL names a public destination before a
transport is given it, as `validate_public_url` of RoomKit's Python
implementation does, in the same words:

```rust
use roomkit_providers_http::{ValidatePublicUrlOptions, validate_public_url};

let options = ValidatePublicUrlOptions::default();
assert_eq!(
    validate_public_url("http://127.1/", &options),
    Err("URL host '127.1' resolves to loopback address 127.0.0.1".to_owned())
);
```

A URL is refused when its host is this machine by name, an address that is
loopback, private, reserved or multicast, in any of the forms the client's
parser reads (`127.1`, `0177.0.0.1`, `0x7f000001`), or a name one of whose
addresses is. By default a name is resolved and every answer is checked.

The crate also holds what the provider crates build their HTTP on, hidden
from its documentation: a bounded reader of server-sent events and the TLS
configuration their clients take.

It depends on neither Python nor RoomKit, nor on `roomkit-providers-core`,
and on no HTTP client: it reads a URL with the `url` crate, the parser
`reqwest` reads one with.

## What it does not guard

- **A name the client resolves again.** The function checks the answers it
  gets and returns only the URL. The client then resolves the name again,
  and may be given other addresses (DNS rebinding): nothing returned lets a
  caller pin the connection to an address that was checked. A caller that
  pins a connection resolves the name itself and checks the address it dials
  by passing it as a URL's host (`http://<address>/`, an IPv6 address in
  brackets): nothing else public judges an address.
- **A client behind a proxy.** The proxy resolves the name, not the client:
  the lookup made here says nothing of what the proxy reaches.
- **Redirects.** Only the URL given is checked. A client that follows
  redirects leaves the guard after the first hop: check each hop, or do not
  follow them.
- **A name, without resolution.** `resolve_dns: false` checks the URL as it
  is written only. An address is judged. A name is refused in three cases:
  it is one of the names of this machine the function always refuses
  (`localhost`, `ip6-localhost` and `ip6-loopback`, in any case, with or
  without trailing dots); the parser refuses the host, as said below; or it
  ends in dots and, without them, spells an address that is not public. Any
  other name is accepted unchecked, `sub.localhost` and
  `localhost.localdomain` among them. It is for tests, and for a caller that
  resolves a name and checks its addresses itself.

It also blocks: a name is resolved synchronously, by the system's resolver,
with no timeout of its own. The call blocks its thread until the resolver
answers, an async executor's thread included: call it from a blocking task.

## How a host is read, and where it differs from Python's

The host is read by the `url` crate. A host that parser reads as an address
is judged as the address the client dials, for a client that reads the URL
with the same crate. `reqwest` 0.13 does: its `Url` is that crate's, and
within the 2.x line cargo keeps one version of it in a build. A client with
another parser may read another host. This holds for an address, not for a
name: a name is resolved here, then again by the client.

A host that ends in two dots or more is a name to the parser, whatever
stands before them. It is refused when, without its dots, it spells an
address that is not public: `127.0.01..` is refused as the address
`127.0.0.1`, as Python refuses it. Otherwise it stays the name the client
resolves: it is resolved here, without its dots, and every answer checked.

The parser reads a host otherwise than Python does in four cases:

- a host whose last label is a number and that is no IPv4 address is
  refused, where Python takes it for a name: `999.1.1.1`, `1.2.3.4.5`,
  `08.1.1.1`, `example.123`, `example.0x10`;
- a hexadecimal part without a digit is read as zero, wherever it stands,
  where Python takes the host for a name: `0x` is judged as the address
  `0.0.0.0`, and `0xf.0x.0x.0x` as `15.0.0.0`, without a lookup;
- a label that starts with `xn--` and is no Punycode is refused, where
  Python takes the host for a name: `xn--a.example`;
- a public address followed by two dots or more is a name, resolved as said
  above, where Python reads the address and asks no resolver: `1.2.3..`,
  `93.184.216.34..`.

A refusal says what Python's says, with three exceptions: a bracketed host
that is no IPv6 address and a pair of brackets in the user information are
refused in one wording each, where Python's depends on the fault; and a
character past ASCII in a port's text is written as it is, where Python
escapes one that is not printable.

The crate's tests replay a table of URLs and addresses that RoomKit's Python
decided (`tests/fixtures/python_public_urls.json`), each name resolved from
the table, never from the network.

## License

`MIT AND Unicode-3.0`.

The crate is under the MIT license (`LICENSE`). Its list of the characters
that Unicode's compatibility forms turn into a delimiter of a URL
(`src/public_url/authority.rs`) is derived from Unicode data, under the
Unicode License V3 (`LICENSE-UNICODE`).
