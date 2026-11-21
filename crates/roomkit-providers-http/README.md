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
loopback, private, reserved or multicast, in any of the forms a resolver
reads, or a name one of whose addresses is. By default a name is resolved,
synchronously, and every answer is checked; `resolve_dns: false` checks the
URL as it is written. Validation observes the answers at the time of the
call: a transport that needs to pin the connection must dial an address it
has checked.

The crate also holds what the provider crates build their HTTP on, hidden
from its documentation: a bounded reader of server-sent events and the TLS
configuration their clients take.

It depends on neither Python nor RoomKit, nor on `roomkit-providers-core`.

## Where it differs from Python's

The host is read by the parser the HTTP client dials with, so that the
address checked is the address dialed. Where that parser reads a host
otherwise than Python does, the URL is refused, and Python takes it for a
name:

- a host whose last label is a number and that is no IPv4 address:
  `999.1.1.1`, `1.2.3.4.5`, `08.1.1.1`, `example.123`, `example.0x10`;
- `0x`, read as the address `0.0.0.0`;
- a label that starts with `xn--` and is no Punycode: `xn--a.example`.

A refusal says what Python's says, with three exceptions: a bracketed host
that is no IPv6 address and a pair of brackets in the user information are
refused in one wording each, where Python's depends on the fault; and a
character past ASCII in a port's text is written as it is, where Python
escapes one that is not printable.

The crate's tests replay a table of URLs and addresses that RoomKit's Python
decided (`tests/fixtures/python_public_urls.json`), each name resolved from
the table, never from the network.

## License

MIT
