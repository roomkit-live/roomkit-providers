#!/usr/bin/env python3
"""Write which proxy the client of RoomKit's Python providers takes, for the client crate's tests.

RoomKit's Python providers send through the HTTP client of their vendor's
SDK, which reads the proxy variables of the environment when it is built.
Each case here is an environment and the URLs of some endpoints; the fixture
records, for each URL, the proxy that client sends through, with the
credentials it gives it, or that it sends directly; or the exception the
client raises when it is built. The Rust test ``python_proxy_cases`` reads
the same environments and requires the same answers.

Nothing is sent: a client is built, asked which transport it takes for a
URL, and dropped.

Run it with a Python interpreter that has ``roomkit`` installed with its
``openai`` extra (``make python-proxy-cases``). The fixture records the
versions it was written from. The interpreter has no ``socksio``, which
RoomKit does not install: a SOCKS proxy is refused there.
"""

from __future__ import annotations

import base64
import json
import os
import platform
from contextlib import contextmanager
from importlib import metadata
from pathlib import Path
from typing import Any

import httpx2
import openai

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURE = REPOSITORY / "crates/roomkit-providers-client/tests/fixtures/python_proxy_cases.json"

PROXY = "http://proxy.example:3128"
ALL = {"ALL_PROXY": PROXY}
HTTP = "http://llm.example/v1"
HTTPS = "https://llm.example/v1"
BOTH = [HTTP, HTTPS]
# The hosts a rule of ``NO_PROXY`` for ``llm.example`` is asked about.
NAMES = [
    "https://llm.example/v1",
    "https://api.llm.example/v1",
    "https://notllm.example/v1",
    "https://llm.example.org/v1",
    "https://LLM.Example/v1",
]

Case = tuple[str, dict[str, str], list[str]]


def no_proxy(name: str, value: str, urls: list[str]) -> Case:
    """A case whose every endpoint goes through one proxy, unless ``NO_PROXY``,
    set to `value`, says otherwise."""
    return (name, {**ALL, "NO_PROXY": value}, urls)


CASES: list[Case] = [
    # Which variable is read.
    ("nothing_is_set", {}, BOTH),
    ("http_proxy_serves_http_alone", {"HTTP_PROXY": PROXY}, BOTH),
    ("https_proxy_serves_https_alone", {"HTTPS_PROXY": PROXY}, BOTH),
    ("all_proxy_serves_both", ALL, BOTH),
    (
        "a_scheme_s_own_proxy_wins_over_all_proxy",
        {"ALL_PROXY": "http://all.example:1", "HTTPS_PROXY": "http://secure.example:2"},
        BOTH,
    ),
    (
        "the_lower_case_name_wins_when_it_comes_first",
        {"http_proxy": "http://lower.example:1", "HTTP_PROXY": "http://upper.example:2"},
        [HTTP],
    ),
    (
        "the_lower_case_name_wins_when_it_comes_last",
        {"HTTP_PROXY": "http://upper.example:2", "http_proxy": "http://lower.example:1"},
        [HTTP],
    ),
    (
        "an_empty_lower_case_name_unsets_the_upper_case_one",
        {"HTTPS_PROXY": "http://upper.example:2", "https_proxy": ""},
        [HTTPS],
    ),
    (
        "an_empty_upper_case_name_leaves_the_lower_case_one",
        {"HTTPS_PROXY": "", "https_proxy": "http://lower.example:1"},
        [HTTPS],
    ),
    ("an_empty_name_alone_is_no_proxy", {"ALL_PROXY": ""}, BOTH),
    ("a_name_in_mixed_case_is_read", {"Https_Proxy": "http://mixed.example:1"}, [HTTPS]),
    (
        "a_name_that_ends_in_lower_case_wins_as_the_lower_case_one",
        {"HTTPS_proxy": "http://mixed.example:1", "HTTPS_PROXY": "http://upper.example:2"},
        [HTTPS],
    ),
    (
        "the_last_of_two_upper_case_spellings_wins",
        {"Https_Proxy": "http://first.example:1", "HTTPS_PROXY": "http://last.example:2"},
        [HTTPS],
    ),
    (
        "a_name_that_only_resembles_one_is_not_read",
        {"XHTTPS_PROXY": PROXY, "HTTPSPROXY": PROXY},
        BOTH,
    ),
    # The rule of a CGI program.
    ("a_cgi_program_drops_http_proxy", {"HTTP_PROXY": PROXY, "REQUEST_METHOD": "GET"}, BOTH),
    (
        "a_cgi_program_set_to_nothing_drops_it_too",
        {"HTTP_PROXY": PROXY, "REQUEST_METHOD": ""},
        BOTH,
    ),
    (
        "a_cgi_program_drops_http_proxy_in_mixed_case",
        {"Http_Proxy": PROXY, "REQUEST_METHOD": "GET"},
        BOTH,
    ),
    (
        "a_cgi_program_keeps_the_lower_case_http_proxy",
        {"http_proxy": PROXY, "REQUEST_METHOD": "GET"},
        BOTH,
    ),
    (
        "a_cgi_program_keeps_https_proxy_and_all_proxy",
        {
            "HTTPS_PROXY": "http://secure.example:2",
            "ALL_PROXY": "http://all.example:1",
            "HTTP_PROXY": PROXY,
            "REQUEST_METHOD": "GET",
        },
        BOTH,
    ),
    # How a proxy is written.
    ("a_proxy_without_a_scheme_is_an_http_one", {"HTTPS_PROXY": "proxy.example:3128"}, [HTTPS]),
    ("a_proxy_without_a_port_takes_its_scheme_s", {"HTTPS_PROXY": "http://proxy.example"}, [HTTPS]),
    ("a_proxy_may_be_reached_over_tls", {"HTTPS_PROXY": "https://proxy.example"}, BOTH),
    ("a_proxy_s_scheme_is_read_in_any_case", {"HTTPS_PROXY": "HTTP://Proxy.Example:3128"}, [HTTPS]),
    ("a_proxy_is_an_ipv4_address", {"HTTPS_PROXY": "http://10.0.0.7:3128"}, [HTTPS]),
    ("a_proxy_is_an_ipv6_address", {"HTTPS_PROXY": "http://[fd00::7]:3128"}, [HTTPS]),
    (
        "a_proxy_s_path_is_not_part_of_it",
        {"HTTPS_PROXY": "http://proxy.example:3128/a?b#c"},
        [HTTPS],
    ),
    (
        "a_proxy_carries_its_credentials",
        {"ALL_PROXY": "http://user:placeholder-word@proxy.example:3128"},
        BOTH,
    ),
    (
        "a_proxy_s_credentials_are_decoded",
        {"HTTPS_PROXY": "http://us%65r:pass%40word%3A@proxy.example:3128"},
        [HTTPS],
    ),
    ("a_proxy_s_user_alone_is_sent", {"HTTPS_PROXY": "http://user@proxy.example:3128"}, [HTTPS]),
    (
        "a_proxy_s_empty_password_is_sent",
        {"HTTPS_PROXY": "http://user:@proxy.example:3128"},
        [HTTPS],
    ),
    (
        "a_proxy_s_password_alone_is_sent",
        {"HTTPS_PROXY": "http://:word@proxy.example:3128"},
        [HTTPS],
    ),
    (
        "a_proxy_s_empty_credentials_are_none",
        {"HTTPS_PROXY": "http://:@proxy.example:3128"},
        [HTTPS],
    ),
    (
        "a_proxy_s_credentials_end_at_the_last_at_sign",
        {"HTTPS_PROXY": "http://user@realm:word@proxy.example:3128"},
        [HTTPS],
    ),
    ("a_socks5_proxy_is_refused", {"HTTPS_PROXY": "socks5://proxy.example:1080"}, [HTTPS]),
    ("a_socks5h_proxy_is_refused", {"ALL_PROXY": "socks5h://proxy.example:1080"}, [HTTPS]),
    ("a_socks4_proxy_is_refused", {"HTTPS_PROXY": "socks4://proxy.example:1080"}, [HTTPS]),
    ("a_proxy_of_another_scheme_is_refused", {"HTTP_PROXY": "ftp://proxy.example:21"}, [HTTPS]),
    (
        "a_proxy_whose_port_is_no_number_is_refused",
        {"HTTPS_PROXY": "http://proxy.example:port"},
        [HTTPS],
    ),
    (
        "a_proxy_written_between_spaces_is_refused",
        {"HTTPS_PROXY": " http://proxy.example:3128 "},
        [HTTPS],
    ),
    (
        "a_proxy_with_a_control_character_is_refused",
        {"HTTPS_PROXY": "http://proxy.example\t:3128"},
        [HTTPS],
    ),
    (
        "a_proxy_whose_address_is_none_is_refused",
        {"HTTPS_PROXY": "http://10.0.0.999:3128"},
        [HTTPS],
    ),
    (
        "a_proxy_whose_ipv6_address_is_none_is_refused",
        {"HTTPS_PROXY": "http://[fd00::g]:3128"},
        [HTTPS],
    ),
    ("a_proxy_without_a_host", {"HTTPS_PROXY": "http://"}, [HTTPS]),
    (
        "a_proxy_whose_host_is_not_ascii",
        {"HTTPS_PROXY": "http://b\u00fccher.example:3128"},
        [HTTPS],
    ),
    ("a_proxy_whose_port_has_a_sign", {"HTTPS_PROXY": "http://proxy.example:+3128"}, [HTTPS]),
    ("a_proxy_whose_port_is_past_the_last", {"HTTPS_PROXY": "http://proxy.example:70000"}, [HTTPS]),
    # NO_PROXY: a name.
    no_proxy("a_name_exempts_itself_and_what_is_under_it", "llm.example", NAMES),
    no_proxy("a_name_is_read_in_any_case", "LLM.Example", NAMES),
    no_proxy("a_name_after_a_dot_exempts_what_is_under_it_alone", ".llm.example", NAMES),
    no_proxy(
        "a_name_of_one_label_exempts_what_ends_with_it", "example", [*NAMES, "https://anexample/v1"]
    ),
    no_proxy("a_star_before_a_dot_exempts_nothing", "*.llm.example", NAMES),
    no_proxy("a_star_before_a_name_exempts_nothing", "*llm.example", NAMES),
    no_proxy("a_star_inside_a_name_exempts_nothing", "llm.*", NAMES),
    no_proxy(
        "a_name_that_ends_in_a_dot_is_another_name",
        "llm.example.",
        ["https://llm.example/v1", "https://llm.example./v1", "https://api.llm.example./v1"],
    ),
    no_proxy(
        "an_endpoint_that_ends_in_a_dot_is_another_name",
        "llm.example",
        ["https://llm.example./v1"],
    ),
    no_proxy(
        "an_internationalized_endpoint_is_matched_by_its_labels",
        "example",
        ["https://api.xn--bcher-kva.example/v1"],
    ),
    no_proxy(
        "a_label_is_matched_whole",
        "kva.example",
        ["https://api.xn--bcher-kva.example/v1", "https://kva.example/v1"],
    ),
    # NO_PROXY: everything.
    no_proxy("a_star_exempts_everything", "*", [*BOTH, "http://10.0.0.1/v1"]),
    no_proxy("a_star_among_others_exempts_everything", "other.example, *", BOTH),
    (
        "a_star_keeps_a_proxy_that_does_not_read_from_being_read",
        {"HTTPS_PROXY": "ftp://proxy.example", "NO_PROXY": "*, [::1]"},
        BOTH,
    ),
    # NO_PROXY: an address.
    no_proxy(
        "an_address_exempts_itself",
        "10.0.0.1",
        [
            "http://10.0.0.1/v1",
            "http://10.0.0.1:8080/v1",
            "http://10.0.0.11/v1",
            "http://110.0.0.1/v1",
        ],
    ),
    no_proxy(
        "a_range_exempts_the_one_address_it_is_written_with",
        "10.0.0.0/8",
        ["http://10.0.0.0/v1", "http://10.1.2.3/v1", "http://11.0.0.1/v1"],
    ),
    no_proxy(
        "a_range_written_with_a_host_exempts_that_host",
        "192.168.1.7/24",
        ["http://192.168.1.7/v1", "http://192.168.1.8/v1"],
    ),
    no_proxy(
        "the_loopback_address_is_not_the_name_localhost",
        "127.0.0.1",
        ["http://127.0.0.1:8000/v1", "http://localhost:8000/v1", "http://127.0.0.2/v1"],
    ),
    no_proxy(
        "an_address_with_a_port_exempts_that_port",
        "10.0.0.1:8080",
        ["http://10.0.0.1:8080/v1", "http://10.0.0.1/v1", "http://10.0.0.1:8081/v1"],
    ),
    no_proxy("an_ipv6_address_exempts_itself", "::1", ["http://[::1]:8000/v1", "http://[::2]/v1"]),
    no_proxy(
        "an_ipv6_range_exempts_the_one_address_it_is_written_with",
        "fd00::/8",
        ["http://[fd00::]/v1", "http://[fd00::1]/v1"],
    ),
    no_proxy("an_ipv6_address_in_brackets_is_refused", "[::1]", [HTTP]),
    no_proxy("an_address_that_is_none_is_a_name", "10.0.0.999", ["http://10.0.0.1/v1"]),
    # NO_PROXY: localhost.
    no_proxy(
        "localhost_exempts_itself_alone",
        "localhost",
        [
            "http://localhost:8000/v1",
            "http://LOCALHOST/v1",
            "http://api.localhost/v1",
            "http://127.0.0.1/v1",
        ],
    ),
    no_proxy(
        "localhost_is_read_in_any_case",
        "LocalHost",
        ["http://localhost/v1", "http://api.localhost/v1"],
    ),
    no_proxy(
        "localhost_with_a_port_exempts_that_port_and_what_is_under_it",
        "localhost:8000",
        ["http://localhost:8000/v1", "http://localhost/v1", "http://api.localhost:8000/v1"],
    ),
    # NO_PROXY: a port.
    no_proxy(
        "a_name_with_a_port_exempts_that_port",
        "llm.example:8443",
        [
            "https://llm.example:8443/v1",
            "http://llm.example:8443/v1",
            "https://api.llm.example:8443/v1",
            "https://llm.example/v1",
        ],
    ),
    no_proxy(
        "a_scheme_s_own_port_is_never_the_port_of_its_endpoint",
        "llm.example:443",
        ["https://llm.example/v1", "https://llm.example:443/v1", "http://llm.example:443/v1"],
    ),
    no_proxy("a_port_left_empty_is_no_port", "llm.example:", BOTH),
    no_proxy(
        "a_port_alone_exempts_every_host_on_it", ":8443", ["https://llm.example:8443/v1", HTTPS]
    ),
    no_proxy(
        "a_port_written_with_zeros_is_its_number",
        "llm.example:08443",
        ["https://llm.example:8443/v1"],
    ),
    no_proxy("a_port_that_is_no_number_is_refused", "llm.example:port", BOTH),
    no_proxy("a_port_with_a_sign", "llm.example:+8443", ["https://llm.example:8443/v1"]),
    no_proxy("a_port_past_the_last_is_no_endpoint_s", "llm.example:70000", BOTH),
    # NO_PROXY: an entry written as a URL.
    no_proxy(
        "an_entry_with_a_scheme_exempts_that_scheme_and_that_host",
        "https://llm.example",
        [*BOTH, "https://api.llm.example/v1"],
    ),
    no_proxy("an_entry_s_scheme_is_read_in_any_case", "HTTPS://llm.example", BOTH),
    no_proxy(
        "an_entry_s_port_under_a_scheme_in_upper_case_is_kept",
        "HTTPS://llm.example:443",
        ["https://llm.example/v1", "https://llm.example:443/v1"],
    ),
    no_proxy(
        "an_entry_under_all_exempts_every_scheme",
        "all://llm.example:8443",
        ["https://llm.example:8443/v1", HTTPS],
    ),
    no_proxy("an_entry_may_exempt_what_is_under_a_name", "all://*.llm.example", NAMES),
    no_proxy(
        "an_entry_may_exempt_a_name_and_what_is_under_it", "https://*llm.example", [*NAMES, HTTP]
    ),
    no_proxy("an_entry_that_is_a_scheme_exempts_it", "https://", BOTH),
    (
        "an_entry_that_is_a_scheme_takes_the_place_of_its_proxy",
        {
            "HTTPS_PROXY": "http://secure.example:2",
            "HTTP_PROXY": "ftp://proxy.example",
            "NO_PROXY": "http://",
        },
        BOTH,
    ),
    no_proxy("an_entry_that_is_all_takes_the_place_of_all_proxy", "all://", BOTH),
    (
        "an_entry_that_is_all_leaves_a_scheme_s_own_proxy",
        {"ALL_PROXY": PROXY, "HTTPS_PROXY": "http://secure.example:2", "NO_PROXY": "all://"},
        BOTH,
    ),
    no_proxy("an_entry_for_every_host_comes_after_the_proxies", "all://*", BOTH),
    no_proxy("an_entry_without_a_scheme_s_name_reads_from_its_slashes", "://llm.example", BOTH),
    no_proxy(
        "an_entry_for_an_address_in_brackets", "all://[::1]", ["http://[::1]/v1", "http://[::2]/v1"]
    ),
    no_proxy("an_entry_whose_address_is_none_is_refused", "https://10.0.0.999", BOTH),
    no_proxy(
        "an_entry_with_user_information_exempts_its_host_alone",
        "user@llm.example",
        ["https://llm.example/v1", "https://api.llm.example/v1"],
    ),
    no_proxy(
        "an_entry_s_path_is_not_part_of_it",
        "llm.example/path",
        ["https://llm.example/v1", "https://api.llm.example/v1"],
    ),
    # NO_PROXY: how its entries are told apart.
    no_proxy(
        "entries_are_trimmed",
        " llm.example , other.example ,,",
        [HTTPS, "https://other.example/v1"],
    ),
    no_proxy(
        "a_semicolon_separates_nothing",
        "llm.example;other.example",
        [HTTPS, "https://other.example/v1"],
    ),
    no_proxy(
        "a_space_separates_nothing",
        "llm.example other.example",
        [HTTPS, "https://other.example/v1"],
    ),
    no_proxy("an_entry_no_host_spells_exempts_nothing", "<local>", BOTH),
    no_proxy("an_entry_that_is_not_ascii_is_refused", "b\u00fccher.example", BOTH),
    no_proxy("an_entry_in_punycode_is_refused", "xn--bcher-kva.example", BOTH),
    no_proxy("an_entry_with_a_control_character_is_refused", "llm.example\x7f", BOTH),
    (
        "the_lower_case_no_proxy_wins",
        {**ALL, "no_proxy": "llm.example", "NO_PROXY": "other.example"},
        [HTTPS, "https://other.example/v1"],
    ),
    (
        "an_empty_lower_case_no_proxy_unsets_the_upper_case_one",
        {**ALL, "no_proxy": "", "NO_PROXY": "llm.example"},
        [HTTPS],
    ),
    ("no_proxy_alone_changes_nothing", {"NO_PROXY": "llm.example"}, BOTH),
    # What an endpoint is read as.
    no_proxy(
        "an_endpoint_s_own_port_is_no_port",
        "llm.example:8080",
        ["http://llm.example:80/v1", "https://llm.example:443/v1", "http://llm.example:8080/v1"],
    ),
    no_proxy(
        "an_ipv6_endpoint_is_the_text_it_is_written_in",
        "::1",
        ["http://[::1]/v1", "http://[0:0:0:0:0:0:0:1]/v1"],
    ),
    no_proxy(
        "an_ipv4_endpoint_is_the_text_it_is_written_in",
        "127.0.0.1",
        ["http://127.0.0.1/v1", "http://127.1/v1"],
    ),
]

# What every case is read without: the variables of whoever runs the script.
READ = ("_proxy", "REQUEST_METHOD")


@contextmanager
def environment(variables: dict[str, str]) -> Any:
    """The environment a case is read in: `variables`, in their order, after
    the variables of the machine that are none of a proxy's."""
    kept = dict(os.environ)
    for name in list(os.environ):
        if name.lower().endswith(READ[0]) or name == READ[1]:
            del os.environ[name]
    os.environ.update(variables)
    try:
        yield
    finally:
        os.environ.clear()
        os.environ.update(kept)


def taken(client: httpx2.AsyncClient, url: str) -> dict[str, Any]:
    """Where `client` sends a request for `url`: the proxy it goes through,
    with the credentials it is given, or neither when it goes directly."""
    transport = client._transport_for_url(httpx2.URL(url))
    if transport is client._transport:
        return {"url": url, "proxy": None, "credentials": None}
    pool = transport._pool
    origin = pool._proxy_url
    host = origin.host.decode("ascii")
    port = origin.port or {b"http": 80, b"https": 443}[origin.scheme]
    credentials = None
    for name, value in pool._proxy_headers:
        if name.lower() == b"proxy-authorization":
            scheme, _, encoded = value.partition(b" ")
            assert scheme == b"Basic", value
            credentials = base64.b64decode(encoded).decode("utf-8")
    return {
        "url": url,
        "proxy": {"scheme": origin.scheme.decode("ascii"), "host": host, "port": port},
        "credentials": credentials,
    }


def decided(name: str, variables: dict[str, str], urls: list[str]) -> dict[str, Any]:
    """What Python's client decides of one case."""
    case: dict[str, Any] = {"name": name, "environment": list(variables.items())}
    with environment(variables):
        try:
            # The client RoomKit's OpenAI provider is given when it names
            # no transport: the SDK's own.
            client = openai.DefaultAsyncHttpxClient()
        except Exception as error:
            return {**case, "refused": type(error).__name__, "asked": []}
        return {**case, "refused": None, "asked": [taken(client, url) for url in urls]}


def main() -> None:
    names = [name for name, _, _ in CASES]
    assert len(names) == len(set(names)), "two cases share a name"
    fixture = {
        "generator": "scripts/generate_python_proxy_cases.py",
        "roomkit_version": metadata.version("roomkit"),
        "openai_version": metadata.version("openai"),
        "httpx2_version": metadata.version("httpx2"),
        "python_version": platform.python_version(),
        "cases": [decided(*case) for case in CASES],
    }
    FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    FIXTURE.write_text(json.dumps(fixture, indent=1, ensure_ascii=True) + "\n", encoding="utf-8")
    refused = sum(1 for case in fixture["cases"] if case["refused"])
    print(
        f"Wrote {FIXTURE.relative_to(REPOSITORY)}: {len(CASES)} cases, {refused} of them "
        f"refused, from roomkit {fixture['roomkit_version']}, openai "
        f"{fixture['openai_version']} and httpx2 {fixture['httpx2_version']}"
    )


if __name__ == "__main__":
    main()
