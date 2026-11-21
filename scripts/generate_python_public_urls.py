#!/usr/bin/env python3
"""Write what RoomKit's Python decides of a table of URLs, for the HTTP crate's tests.

Each case holds a URL, the options ``validate_public_url`` is given, and what
it answers: the URL taken, or the words of its ``ValueError``, with the names
it asked a resolver for. The Rust test ``python_public_urls`` replays the
table and requires the same answers.

No lookup leaves this script: ``socket.getaddrinfo`` is replaced by a table of
answers, and a name the table does not hold stops the script. The fixture
carries that table, so the Rust test resolves from it too and needs no
network either.

Run it with a Python interpreter that has ``roomkit`` installed
(``make python-public-urls``). The fixture records the RoomKit and Python
versions it was written from: the address policy follows Python's
``ipaddress`` module. It also records, from Python's Unicode data, the
characters ``urlsplit`` refuses in an authority, which the crate lists.

That list is derived from Unicode data, under the Unicode License V3. The
script writes that license's notice as the license file the crate ships
beside its own, ``LICENSE-UNICODE``.
"""

from __future__ import annotations

import json
import platform
import socket
import unicodedata
from pathlib import Path
from typing import Any

import roomkit
from roomkit.providers import validate_public_url
from unicode_notice import UNICODE_NOTICE

REPOSITORY = Path(__file__).resolve().parent.parent
CRATE = REPOSITORY / "crates/roomkit-providers-http"
FIXTURE = CRATE / "tests/fixtures/python_public_urls.json"
LICENSE = CRATE / "LICENSE-UNICODE"

PUBLIC = "93.184.216.34"
PUBLIC_V6 = "2606:4700:4700::1111"
LONG_LABEL = "a" * 64

# The addresses each name of the table resolves to; None for a name with no
# answer.
ANSWERS: dict[str, list[str] | None] = {
    "service.example": [PUBLIC],
    "xn--caf-dma.example": [PUBLIC],
    "xn--a.example": [PUBLIC],
    "a_b.example": [PUBLIC],
    "-a.example": [PUBLIC],
    "v6.example": [PUBLIC_V6],
    "both.example": [PUBLIC, PUBLIC_V6],
    "mixed.example": [PUBLIC, "127.0.0.1"],
    "mixed-v6.example": [PUBLIC, "::1"],
    "inside.example": ["10.0.0.5"],
    f"{LONG_LABEL}.example": [PUBLIC],
    "service..example": [PUBLIC],
    "sub.localhost": ["127.0.0.1"],
    "localhost.localdomain": ["::1"],
    "nowhere.example": None,
    # Texts Python reads as names, since they are no IPv4 address.
    "4294967296": None,
    "999.1.1.1": None,
    "1.2.3.4.5": None,
    "08.1.1.1": None,
    "example.123": None,
    "example.0x10": None,
    "0x": None,
    "0xf.0x.0x.0x": None,
    "15.0x": None,
    "192.10.0x.0x": None,
    "16.0x": ["10.0.0.5"],
    # Texts a resolver reads as the address they spell. Python reads the
    # address itself and asks for none of them; the crate asks for the ones
    # that dots follow.
    "1.2.3": ["1.2.0.3"],
    PUBLIC: [PUBLIC],
}

OFFLINE = {"resolve_dns": False}
SERVICE = {"allowed_schemes": ("service",)}
HTTPS_ALONE = {"allowed_schemes": ("https",)}

# Each case: its name, the URL, and the options that are not the defaults.
URLS: list[tuple[str, str, dict[str, Any]]] = [
    # A public destination, and the spellings that name the same one.
    ("https", "https://service.example/", {}),
    ("http", "http://service.example/", {}),
    ("path_query_and_fragment", "https://service.example:8443/A?q=1#fragment", {}),
    ("upper_case", "HTTPS://Service.EXAMPLE/Path", {}),
    ("user_and_password", "https://user:pass@service.example/", {}),
    ("escaped_user", "https://user%40mail:p%40ss@service.example/", {}),
    ("user_past_ascii", "https://caf\u00e9@service.example/", {}),
    ("user_empty", "https://@service.example/", {}),
    ("user_and_password_empty", "https://:@service.example/", {}),
    ("password_with_colons", "https://user:pa:ss@service.example/", {}),
    ("trailing_dot", "https://service.example./", {}),
    ("trailing_dots", "https://service.example../", {}),
    ("leading_space", " https://service.example/", {}),
    ("leading_tab", "\thttps://service.example/", {}),
    ("leading_line_break", "\r\nhttps://service.example/", {}),
    ("leading_no_break_space", "\u00a0https://service.example/", {}),
    ("leading_vertical_tab", "\x0bhttps://service.example/", {}),
    ("leading_form_feed", "\x0chttps://service.example/", {}),
    ("path_past_ascii", "https://service.example/caf\u00e9?q=\u00e9t\u00e9", {}),
    ("underscore_in_a_name", "https://a_b.example/", {}),
    ("hyphen_first_in_a_name", "https://-a.example/", {}),
    ("punycode_in_upper_case", "https://XN--CAF-DMA.example/", {}),
    ("punycode_that_decodes_to_nothing", "https://xn--a.example/", {}),
    ("label_past_63_characters", f"https://{LONG_LABEL}.example/", {}),
    ("empty_label_in_a_name", "https://service..example/", {}),
    ("name_not_resolved", "https://service.example/", OFFLINE),
    ("unknown_name_not_resolved", "https://nowhere.example/", OFFLINE),
    # The port a name is resolved at.
    ("port_empty", "https://service.example:/", {}),
    ("port_zero", "https://service.example:0/", {}),
    ("port_with_zeros", "https://service.example:00443/", {}),
    ("port_of_the_other_scheme", "http://service.example:443/", {}),
    ("port_largest", "https://service.example:65535/", {}),
    ("port_past_the_largest", "https://service.example:65536/", {}),
    ("port_past_any_integer", "https://service.example:99999999999999999999/", {}),
    ("port_no_number", "https://service.example:invalid/", {}),
    ("port_no_number_not_resolved", "https://service.example:invalid/", OFFLINE),
    ("port_of_an_address_no_number", f"https://{PUBLIC}:invalid/", {}),
    ("port_of_an_address_past_the_largest", f"https://{PUBLIC}:65536/", OFFLINE),
    ("port_two_colons", "https://service.example:1:2/", {}),
    ("port_signed", "https://service.example:+80/", {}),
    ("port_negative", "https://service.example:-1/", {}),
    ("port_with_a_letter", "https://service.example:8080x/", {}),
    ("port_with_a_quote", "https://service.example:it's/", {}),
    ("port_past_ascii", "https://service.example:\u00e9/", {}),
    ("port_no_break_space", "https://service.example:\u00a0/", {}),
    # Schemes.
    ("scheme_not_allowed", "ftp://service.example/", {}),
    ("scheme_http_where_https_alone", "http://service.example/", HTTPS_ALONE),
    ("scheme_https_where_https_alone", "https://service.example/", HTTPS_ALONE),
    ("scheme_none_allowed", "https://service.example/", {"allowed_schemes": ()}),
    (
        "scheme_allowed_in_upper_case",
        "https://service.example/",
        {"allowed_schemes": ("HTTPS",)},
    ),
    ("scheme_of_an_application", "service://service.example/resource", SERVICE),
    ("scheme_of_an_application_port", "service://service.example:443/resource", SERVICE),
    ("scheme_of_an_application_loopback", "service://127.1/resource", SERVICE),
    (
        "scheme_with_signs",
        "svc+tls.v1://service.example/",
        {"allowed_schemes": ("svc+tls.v1",)},
    ),
    ("scheme_refused_after_its_authority", "ftp://\\evil.example/", {}),
    # No scheme, or no host.
    ("no_scheme", "service.example", {}),
    ("empty", "", {}),
    ("one_slash", "https:/service.example", {}),
    ("no_authority", "https://", {}),
    ("no_authority_before_a_path", "https:///path", {}),
    ("no_authority_before_a_query", "https://?q=1", {}),
    ("no_authority_before_a_fragment", "https://#fragment", {}),
    ("no_scheme_before_the_slashes", "://service.example/", {}),
    ("scheme_starting_with_a_digit", "1https://service.example/", {}),
    ("scheme_with_a_slash", "a/b://service.example/", {}),
    ("user_and_no_host", "https://user@/", {}),
    ("port_and_no_host", "https://:80/", {}),
    ("host_of_one_dot", "https://./", {}),
    ("host_of_dots", "https://../", {}),
    # An authority that reads two ways.
    ("backslash_first", "https://\\evil.example/", {}),
    ("backslash_before_an_at", "https://service.example\\@evil.example/", {}),
    ("space_in_a_name", "https://service .example/", {}),
    ("tab_in_a_name", "https://service\t.example/", {}),
    ("delete_in_a_name", "https://service\x7f.example/", {}),
    ("two_ats", "https://one@two@service.example/", {}),
    ("escape_in_a_name", "https://service%2Eexample/", {}),
    ("name_past_ascii", "https://caf\u00e9.example/", {}),
    ("name_of_a_kelvin_sign", "https://\u212a.example/", {}),
    ("fragment_before_an_at", "https://service.example#@evil.example/", {}),
    ("query_before_an_at", "https://service.example?@evil.example/", {}),
    # Brackets.
    ("bracket_text_after", "https://[::1]x/", {}),
    ("bracket_no_address", "https://[::g]/", {}),
    ("bracket_not_closed", "https://[::1", {}),
    ("bracket_empty", "https://[]/", {}),
    ("bracket_zone", "https://[fe80::1%25eth0]/", {}),
    ("bracket_future_address", "https://[v1.x]/", {}),
    ("bracket_two_parts", "https://[1:2]/", {}),
    ("bracket_ipv4", "https://[127.0.0.1]/", {}),
    ("bracket_two_runs", "https://[1::2::3]/", {}),
    ("bracket_port_no_number", "https://[::1]:x/", {}),
    ("bracket_port_past_the_largest", "https://[::1]:99999/", {}),
    ("bracket_port_empty", "https://[::1]:/", {}),
    ("bracket_port", f"https://[{PUBLIC_V6}]:8443/", {}),
    ("bracket_in_upper_case", "https://[2001:DB8::1]/", {}),
    ("bracket_in_full", "https://[0:0:0:0:0:0:0:1]/", {}),
    ("bracket_open_in_the_user", "https://us[er@service.example/", {}),
    ("bracket_pair_in_the_user", "https://us[e]r@service.example/", {}),
    ("bracket_pair_in_the_port", "https://service.example:[1]/", {}),
    ("bracket_pair_in_the_user_of_an_address", f"https://us[e]r@[{PUBLIC_V6}]/", {}),
    # A character Unicode's compatibility forms turn into a delimiter.
    ("user_with_a_wide_number_sign", "https://user\uff03name@service.example/", {}),
    ("user_with_a_wide_at", "https://us\uff20er@service.example/", {}),
    ("user_with_an_account_of_sign", "https://it's\u2100@service.example/", {}),
    ("user_with_a_wide_letter", "https://\uff21@service.example/", {}),
    ("port_with_a_wide_colon", "https://service.example:\uff1a80/", {}),
    ("wide_colon_before_the_scheme_is_judged", "ftp://us\uff1aer@service.example/", {}),
    # This machine, by name.
    ("localhost", "http://localhost/", {}),
    ("localhost_in_upper_case", "https://LOCALHOST/", {}),
    ("localhost_with_a_dot", "https://localhost./", {}),
    ("localhost_with_dots", "https://localhost../", {}),
    ("localhost_not_resolved", "https://localhost/", OFFLINE),
    ("localhost_of_an_application", "service://localhost/resource", SERVICE),
    ("ip6_localhost", "http://ip6-localhost/", {}),
    ("ip6_loopback", "http://ip6-loopback:8080/", {}),
    ("localhost_after_its_port", "https://localhost:99999/", {}),
    ("name_under_localhost", "https://sub.localhost/", {}),
    ("name_starting_with_localhost", "https://localhost.localdomain/", {}),
    ("name_under_localhost_not_resolved", "https://sub.localhost/", OFFLINE),
    (
        "name_starting_with_localhost_not_resolved",
        "https://localhost.localdomain/",
        OFFLINE,
    ),
    # An IPv4 address in the forms a resolver reads.
    ("ipv4_two_parts", "http://127.1/", {}),
    ("ipv4_one_number", "http://2130706433/", {}),
    ("ipv4_octal", "http://0177.0.0.1/", {}),
    ("ipv4_octal_one_number", "http://017700000001/", {}),
    ("ipv4_hexadecimal", "http://0x7f000001/", {}),
    ("ipv4_hexadecimal_in_upper_case", "http://0x7F.1/", {}),
    ("ipv4_hexadecimal_parts", "http://0x7f.0.0.0x1/", {}),
    ("ipv4_with_a_dot", "http://127.0.0.1./", {}),
    ("ipv4_three_parts_public", "http://1.2.3/", {}),
    ("ipv4_three_parts_private", "http://192.168.1/", {}),
    ("ipv4_two_parts_private", "http://10.1/", {}),
    ("ipv4_largest", "http://4294967295/", {}),
    ("ipv4_zero", "http://0/", {}),
    ("ipv4_octal_public", "http://010.0.0.1/", {}),
    ("ipv4_octal_private", "http://0300.0250.0.1/", {}),
    ("ipv4_hexadecimal_mark_in_upper_case", "http://0X7F.1/", {}),
    ("ipv4_with_dots", "http://127.0.0.1../", {}),
    ("ipv4_public_with_a_dot", f"http://{PUBLIC}./", {}),
    # An address followed by dots: the dots dropped, Python reads the address
    # and asks no resolver.
    ("ipv4_short_with_a_dot_not_resolved", "http://127.0.01./", OFFLINE),
    ("ipv4_short_with_dots_not_resolved", "http://127.0.01../", OFFLINE),
    ("ipv4_short_with_three_dots_not_resolved", "http://127.0.01.../", OFFLINE),
    ("ipv4_short_with_dots", "http://127.0.01../", {}),
    ("ipv4_hexadecimal_part_with_dots_not_resolved", "http://0x7f../", OFFLINE),
    ("ipv4_zero_with_dots_not_resolved", "http://0../", OFFLINE),
    ("ipv4_three_parts_with_dots_not_resolved", "http://192.168.1../", OFFLINE),
    ("ipv4_octal_with_dots_not_resolved", "http://0177.0.0.1../", OFFLINE),
    ("ipv4_short_public_with_dots_not_resolved", "http://1.2.3../", OFFLINE),
    ("ipv4_short_public_with_dots", "http://1.2.3../", {}),
    ("ipv4_public_with_dots", f"http://{PUBLIC}../", {}),
    (
        "ipv4_short_with_dots_of_an_application",
        "service://127.0.01../resource",
        {**SERVICE, **OFFLINE},
    ),
    ("name_with_dots_not_resolved", "http://service.example../", OFFLINE),
    ("ipv6_with_dots_not_resolved", "http://[::1]../", OFFLINE),
    # An IPv6 address in the spellings that name the same one.
    ("ipv6_with_zeros", "https://[::0001]/", {}),
    ("ipv6_run_in_the_middle", "https://[0::1]/", {}),
    ("ipv6_mapped_in_hexadecimal", "https://[::FFFF:7F00:1]/", {}),
    ("ipv6_mapped_public_in_hexadecimal", "https://[::ffff:5db8:d822]/", {}),
    ("ipv6_ipv4_with_a_zero", "https://[::ffff:01.2.3.4]/", {}),
    ("ipv6_ipv4_of_three_parts", "https://[::1.2.3]/", {}),
    ("ipv6_eight_parts", "https://[2606:4700:4700:0:0:0:0:1111]/", {}),
    ("ipv6_nine_parts", "https://[1:2:3:4:5:6:7:8:9]/", {}),
    ("ipv6_part_of_five_digits", "https://[00001::]/", {}),
    # Texts that end in a number and are no IPv4 address: Python reads them
    # as names.
    ("number_past_the_largest", "http://4294967296/", {}),
    ("number_past_the_largest_not_resolved", "http://4294967296/", OFFLINE),
    ("part_past_255", "http://999.1.1.1/", {}),
    ("part_past_255_not_resolved", "http://999.1.1.1/", OFFLINE),
    ("five_parts", "http://1.2.3.4.5/", {}),
    ("five_parts_not_resolved", "http://1.2.3.4.5/", OFFLINE),
    ("octal_with_an_eight", "http://08.1.1.1/", {}),
    ("octal_with_an_eight_not_resolved", "http://08.1.1.1/", OFFLINE),
    ("part_past_255_with_dots_not_resolved", "http://999.1.1.1../", OFFLINE),
    ("part_past_255_with_dots", "http://999.1.1.1../", {}),
    ("name_ending_in_a_number", "http://example.123/", {}),
    ("name_ending_in_a_number_not_resolved", "http://example.123/", OFFLINE),
    ("hexadecimal_of_no_digit", "http://0x/", {}),
    ("hexadecimal_of_no_digit_not_resolved", "http://0x/", OFFLINE),
    ("hexadecimal_parts_of_no_digit", "http://0xf.0x.0x.0x/", {}),
    ("hexadecimal_parts_of_no_digit_not_resolved", "http://0xf.0x.0x.0x/", OFFLINE),
    ("hexadecimal_parts_of_no_digit_private_not_resolved", "http://0xa.0x.0x.0x/", OFFLINE),
    # The same followed by dots: a name, whatever address it spells without
    # them.
    ("hexadecimal_part_of_no_digit_with_dots", "http://15.0x../", {}),
    ("hexadecimal_part_of_no_digit_with_dots_not_resolved", "http://15.0x../", OFFLINE),
    ("hexadecimal_parts_of_no_digit_with_dots", "http://192.10.0x.0x..../", {}),
    ("hexadecimal_part_of_no_digit_with_dots_answered", "http://16.0x../", {}),
    (
        "hexadecimal_part_of_no_digit_private_with_dots_not_resolved",
        "http://10.0x../",
        OFFLINE,
    ),
    ("name_ending_in_a_hexadecimal_number", "http://example.0x10/", {}),
    ("name_ending_in_a_hexadecimal_number_not_resolved", "http://example.0x10/", OFFLINE),
    # What a name resolves to.
    ("no_answer", "https://nowhere.example/", {}),
    ("answer_ipv6", "https://v6.example/", {}),
    ("answers_of_both_families", "https://both.example/", {}),
    ("answers_one_loopback", "https://mixed.example/", {}),
    ("answers_one_ipv6_loopback", "https://mixed-v6.example/", {}),
    ("answer_private", "https://inside.example:8443/", {}),
    ("answer_private_not_resolved", "https://inside.example:8443/", OFFLINE),
]

# Addresses, each judged as a URL's host and as the answer of a name.
ADDRESSES: list[str] = [
    # Public.
    PUBLIC,
    "1.1.1.1",
    "8.8.8.8",
    # Loopback.
    "127.0.0.1",
    "127.255.255.254",
    # Private use, and the addresses on each side of a range.
    "10.0.0.1",
    "10.255.255.255",
    "9.255.255.255",
    "11.0.0.0",
    "172.15.255.255",
    "172.16.0.1",
    "172.31.255.255",
    "172.32.0.1",
    "192.167.255.255",
    "192.168.0.1",
    "192.169.0.1",
    # Link-local, and the metadata service of a cloud.
    "169.253.255.255",
    "169.254.0.1",
    "169.254.169.254",
    "169.255.0.1",
    # This network.
    "0.0.0.0",
    "0.1.2.3",
    "1.0.0.0",
    # Shared address space.
    "100.63.255.255",
    "100.64.0.1",
    "100.127.255.255",
    "100.128.0.1",
    # Protocol assignments, with their public exceptions.
    "192.0.0.1",
    "192.0.0.8",
    "192.0.0.9",
    "192.0.0.10",
    "192.0.0.11",
    "192.0.0.170",
    "192.0.1.1",
    # Documentation and benchmarking.
    "192.0.2.1",
    "198.17.255.255",
    "198.18.0.1",
    "198.19.255.255",
    "198.20.0.1",
    "198.51.100.7",
    "203.0.113.9",
    # The relay of 6to4.
    "192.88.99.1",
    # Multicast, reserved and broadcast.
    "223.255.255.255",
    "224.0.0.1",
    "239.255.255.255",
    "240.0.0.1",
    "255.255.255.255",
    # IPv6: public.
    PUBLIC_V6,
    "2001:4860:4860::8888",
    "2000::1",
    "2001:200::1",
    "2001:db9::1",
    "2620:fe::fe",
    # Loopback and unspecified.
    "::1",
    "::",
    "::2",
    # IPv4 addresses written in IPv6.
    "::ffff:127.0.0.1",
    "::ffff:10.0.0.1",
    "::ffff:169.254.169.254",
    "::ffff:0.0.0.0",
    f"::ffff:{PUBLIC}",
    "::ffff:192.0.0.9",
    "::127.0.0.1",
    f"::{PUBLIC}",
    "64:ff9b::7f00:1",
    "64:ff9b:1::1",
    "2002:7f00:1::1",
    "2002::1",
    # Protocol assignments, with their public exceptions.
    "2001::1",
    "2001:1::1",
    "2001:1::2",
    "2001:1::3",
    "2001:2::1",
    "2001:3::1",
    "2001:4:112::1",
    "2001:4:113::1",
    "2001:10::1",
    "2001:20::1",
    "2001:3f:ffff::1",
    "2001:40::1",
    "2001:1ff::1",
    # Documentation.
    "2001:db8::1",
    "3fff::1",
    "3fff:fff:ffff::1",
    "3fff:1000::1",
    # Discard, unique local, link-local and site-local.
    "100::1",
    "100:0:0:1::1",
    "fc00::1",
    "fd12:3456:789a::1",
    "fe80::1",
    "febf:ffff::1",
    "fec0::1",
    # Multicast and the reserved blocks.
    "ff02::1",
    "ff00::",
    "1::1",
    "4000::1",
    "5f00::1",
    "8000::1",
    "a000::1",
    "c000::1",
    "e000::1",
    "f000::1",
    "f800::1",
    "fe00::1",
    # The ends of the shared address space, the first address past it, one
    # of its addresses written in IPv6, and the last site-local address:
    # added last, so that the cases above keep their names.
    "100.64.0.0",
    "100.128.0.0",
    "::ffff:100.64.0.1",
    "feff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
]

LOOKUPS: list[list[Any]] = []


def delimiter_lookalikes() -> list[str]:
    """The characters past ASCII whose compatibility form (NFKC) holds a
    delimiter of a URL, which Python's ``urlsplit`` refuses in an authority."""
    return [
        chr(code)
        for code in range(0x80, 0x110000)
        if not 0xD800 <= code < 0xE000
        and any(mark in unicodedata.normalize("NFKC", chr(code)) for mark in "/?#@:")
    ]


def answers(host: str, port: int, *_args: Any, **_kwargs: Any) -> list[tuple[Any, ...]]:
    """What ``socket.getaddrinfo`` answers from the table, a lookup recorded."""
    LOOKUPS.append([host, port])
    if host not in ANSWERS:
        raise AssertionError(f"no answer is written for {host!r}: it would need the network")
    found = ANSWERS[host]
    if found is None:
        raise socket.gaierror(socket.EAI_NONAME, "Name or service not known")
    return [
        (socket.AF_INET6, socket.SOCK_STREAM, 6, "", (address, port, 0, 0))
        if ":" in address
        else (socket.AF_INET, socket.SOCK_STREAM, 6, "", (address, port))
        for address in found
    ]


def address_cases() -> list[tuple[str, str, dict[str, Any]]]:
    """Each address as a URL's host, and as the one answer of a name."""
    cases: list[tuple[str, str, dict[str, Any]]] = []
    for at, address in enumerate(ADDRESSES):
        host = f"[{address}]" if ":" in address else address
        name = f"address-{at}.example"
        ANSWERS[name] = [address]
        cases.append((f"address_{at}_written", f"https://{host}/", OFFLINE))
        cases.append((f"address_{at}_resolved", f"https://{name}/", {}))
    return cases


def decided(name: str, url: str, options: dict[str, Any]) -> dict[str, Any]:
    """What Python decides of ``url`` under ``options``."""
    LOOKUPS.clear()
    try:
        taken = validate_public_url(url, **options)
    except ValueError as refused:
        accepted, error = False, str(refused)
    else:
        if taken != url:
            raise AssertionError(f"{url!r} came back as {taken!r}")
        accepted, error = True, None
    return {
        "name": name,
        "url": url,
        "allowed_schemes": list(options.get("allowed_schemes", ("http", "https"))),
        "resolve_dns": options.get("resolve_dns", True),
        "accepted": accepted,
        "error": error,
        "lookups": list(LOOKUPS),
    }


def main() -> None:
    socket.getaddrinfo = answers
    cases = [decided(*case) for case in [*URLS, *address_cases()]]
    fixture = {
        "generator": "scripts/generate_python_public_urls.py",
        "roomkit_version": roomkit.__version__,
        "python_version": platform.python_version(),
        "unicode_version": unicodedata.unidata_version,
        "delimiter_lookalikes": delimiter_lookalikes(),
        "answers": ANSWERS,
        "cases": cases,
    }
    FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    # Escaped, so that a no-break space or a control character shows.
    FIXTURE.write_text(json.dumps(fixture, indent=4) + "\n", encoding="utf-8")
    LICENSE.write_text(UNICODE_NOTICE, encoding="utf-8")
    taken = sum(case["accepted"] for case in cases)
    print(
        f"Wrote {FIXTURE.relative_to(REPOSITORY)}: {len(cases)} cases, {taken} taken, "
        f"from roomkit {roomkit.__version__} on Python {platform.python_version()}"
    )
    print(f"Wrote {LICENSE.relative_to(REPOSITORY)}")


if __name__ == "__main__":
    main()
