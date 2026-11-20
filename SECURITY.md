# Security policy

## Supported versions

RoomKit Providers is in early development. Security fixes land on the `main`
branch; there are no releases or maintained release branches yet.

## Reporting a vulnerability

Please do not report security issues in public issues, discussions or pull
requests. Report them privately through GitHub's
[private vulnerability reporting](https://github.com/roomkit-live/roomkit-providers/security/advisories/new).

Include what you can of the following:

- the affected provider, crate or package, and the version or commit;
- a description of the issue and its impact;
- the configuration without its key, and the request or the minimal program
  that reproduces it.

We will acknowledge the report, keep you informed while we work on a fix, and
credit you in the advisory unless you prefer otherwise.

## Scope

A provider holds a credential and sends a conversation to a remote service.
What the service answers is treated as untrusted. Reports of particular
interest include:

- an API key sent to a host other than the configured endpoint, a redirect
  included, or appearing in an error, a log, a debug representation or a
  result;
- a request reaching an address the host did not configure;
- a credential or content of one request used for, or returned by, another;
- a panic, or memory or CPU use beyond the documented limits, driven by a
  response or a stream the service sends.

Some things are the host application's responsibility and are not
vulnerabilities by themselves: which endpoint a provider is configured with,
what a conversation contains, and what the application does with the text a
model writes.
