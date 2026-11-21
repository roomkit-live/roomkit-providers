"""What a provider reads of the environment of its process, once, when it
is built: the proxies it sends through and the certificate authorities it
checks a server against, as RoomKit's own provider reads them.

Each test sets the environment, builds its provider, and asks it against
fakes on this machine: a proxy, an endpoint, an endpoint over TLS whose
certificate an authority made for the test signed. The environment is as it
was when the test ends. A test changes it only while no call is under way.
"""

from __future__ import annotations

import os
import unittest
from collections.abc import Iterator
from contextlib import contextmanager
from typing import Any

from fake_endpoint import (
    JSON,
    NO_OPENSSL,
    OPENSSL,
    Authority,
    Fake,
    Tunnel,
    Whole,
    completion,
)
from roomkit.providers.ai import AIContext, AIMessage, ProviderError
from roomkit.providers.openai import OpenAIConfig as RoomKitConfig
from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

HELLO = AIContext(messages=[AIMessage(role="user", content="Hello")])
PATH = "/v1/chat/completions"
# A proxy's credentials: placeholders. And what a client writes of them.
PASSWORD = "placeholder-not-a-password"
CREDENTIALS = f"user:{PASSWORD}"
BASIC = "Basic dXNlcjpwbGFjZWhvbGRlci1ub3QtYS1wYXNzd29yZA=="
# What a provider reads of its environment, beside the names that end in
# `_proxy`, in any case.
READ = ("request_method", "ssl_cert_file", "ssl_cert_dir")
# Why a test that names OpenAI's own endpoint runs only where the tests are
# given no network (`scripts/no-network.sh`): nothing it sends can leave.
NO_NAMESPACE = (
    "this test asks for OpenAI's own endpoint, and runs only where no network is reachable"
)


def is_read(name: str) -> bool:
    return name.lower().endswith("_proxy") or name.lower() in READ


@contextmanager
def environment(**variables: str) -> Iterator[None]:
    """The environment a provider is built in: this process's, without what
    a provider reads of it, and with `variables`. It is put back after."""
    kept = dict(os.environ)
    for name in filter(is_read, kept):
        del os.environ[name]
    os.environ.update(variables)
    try:
        yield
    finally:
        os.environ.clear()
        os.environ.update(kept)


def provider(base_url: str, config: Any = OpenAIConfig, **members: Any) -> OpenAI:
    held = {"api_key": "placeholder-not-a-key", "model": "local-model", "base_url": base_url}
    return OpenAI(config(**held, **members))


def says(said: str) -> Whole:
    return Whole(200, JSON, completion(said))


def refuses(status: int, body: bytes = b"<html>the proxy says no</html>") -> Whole:
    kind = JSON if body.startswith(b"{") else {"content-type": "text/html"}
    return Whole(status, kind, body)


class ProxyTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.proxy = await Fake().start()
        self.endpoint = await Fake().start()
        self.addAsyncCleanup(self.proxy.stop)
        self.addAsyncCleanup(self.endpoint.stop)
        self.through = f"http://{CREDENTIALS}@127.0.0.1:{self.proxy.port}"

    async def test_a_request_for_an_http_endpoint_is_handed_to_the_proxy_the_environment_names(
        self,
    ) -> None:
        self.proxy.answer(says("through the proxy"), says("through the proxy"))
        for config in (OpenAIConfig, RoomKitConfig):
            with self.subTest(config=config.__module__), environment(HTTP_PROXY=self.through):
                asked = provider("http://llm.example/v1", config)
                response = await asked.generate(HELLO)
                await asked.close()
                self.assertEqual(response.content, "through the proxy")
                handed = self.proxy.seen[-1]
                self.assertEqual(
                    (handed.method, handed.target), ("POST", f"http://llm.example{PATH}")
                )
                self.assertEqual(handed.headers["host"], "llm.example")
                self.assertEqual(handed.headers["proxy-authorization"], BASIC)
        self.assertEqual(len(self.proxy.seen), 2)

    async def test_a_host_no_proxy_names_is_reached_directly(self) -> None:
        self.proxy.answer(says("through the proxy"))
        self.endpoint.answer(says("directly"))
        with environment(http_proxy=self.through, NO_PROXY="other.example, 127.0.0.1"):
            asked = provider(f"http://127.0.0.1:{self.endpoint.port}/v1")
            response = await asked.generate(HELLO)
            await asked.close()
        self.assertEqual(response.content, "directly")
        self.assertEqual(self.proxy.seen, [])
        direct = self.endpoint.seen[0]
        self.assertEqual(direct.target, PATH)
        self.assertNotIn("proxy-authorization", direct.headers)

    async def test_the_environment_is_read_once_when_the_provider_is_built(self) -> None:
        other = await Fake().start()
        self.addAsyncCleanup(other.stop)
        self.proxy.answer(says("the first proxy"), says("the first proxy"))
        other.answer(says("the second proxy"))
        self.endpoint.answer(says("directly"))
        url = f"http://127.0.0.1:{self.endpoint.port}/v1"
        with environment(HTTP_PROXY=self.through):
            first = provider(url)
            # The variable changes after the provider was built: the
            # provider keeps the proxy it read, and one built now reads
            # the new one.
            os.environ["HTTP_PROXY"] = f"http://127.0.0.1:{other.port}"
            self.assertEqual((await first.generate(HELLO)).content, "the first proxy")
            second = provider(url)
            self.assertEqual((await second.generate(HELLO)).content, "the second proxy")
            del os.environ["HTTP_PROXY"]
            self.assertEqual((await first.generate(HELLO)).content, "the first proxy")
            third = provider(url)
            self.assertEqual((await third.generate(HELLO)).content, "directly")
            for built in (first, second, third):
                await built.close()
        self.assertEqual((len(self.proxy.seen), len(other.seen)), (2, 1))

    async def test_what_a_proxy_answers_for_itself_is_a_failure_of_the_request(self) -> None:
        # A request handed to the proxy: its answer reads as the endpoint's.
        # A tunnel it refuses: a connection that failed, whatever it said.
        cases = [
            ("http", 407, "the provider answered HTTP 407: Proxy Authentication Required", False),
            ("http", 502, "the provider answered HTTP 502: Bad Gateway", True),
            ("https", 407, "the connection to the provider failed", True),
            ("https", 502, "the connection to the provider failed", True),
        ]
        for scheme, status, said, retryable in cases:
            with self.subTest(scheme=scheme, status=status):
                self.proxy.answer(refuses(status))
                with environment(ALL_PROXY=self.through):
                    asked = provider(f"{scheme}://llm.example/v1")
                    with self.assertRaises(ProviderError) as raised:
                        await asked.generate(HELLO)
                    await asked.close()
                failure = raised.exception
                answered = status if scheme == "http" else None
                label = f"openai ({status})" if answered else "openai"
                self.assertEqual(str(failure), f"{label}: {said}")
                self.assertEqual((failure.retryable, failure.status_code), (retryable, answered))
                method = "POST" if scheme == "http" else "CONNECT"
                self.assertEqual([seen.method for seen in self.proxy.seen], [method])

    async def test_a_proxy_s_redirect_is_not_followed(self) -> None:
        elsewhere = {"location": f"http://127.0.0.1:{self.endpoint.port}/v1"}
        self.proxy.answer(Whole(302, elsewhere, b""))
        self.endpoint.answer(says("elsewhere"))
        with environment(HTTP_PROXY=self.through):
            asked = provider("http://llm.example/v1")
            with self.assertRaises(ProviderError) as raised:
                await asked.generate(HELLO)
            await asked.close()
        self.assertEqual(
            str(raised.exception),
            "openai (302): the endpoint answered with a redirect (302), which is not followed",
        )
        self.assertFalse(raised.exception.retryable)
        self.assertEqual((len(self.proxy.seen), self.endpoint.seen), (1, []))

    async def test_a_proxy_s_credentials_appear_in_no_failure(self) -> None:
        said = f"no way in for {CREDENTIALS}, sent as {BASIC}, password {PASSWORD}"
        body = b'{"error": {"message": "%b"}}' % said.encode()
        self.proxy.answer(refuses(407, body))
        with environment(HTTP_PROXY=self.through):
            asked = provider("http://llm.example/v1")
            with self.assertRaises(ProviderError) as raised:
                await asked.generate(HELLO)
            shown = f"{raised.exception} {raised.exception!r} {asked!r}"
            await asked.close()
        self.assertEqual(
            str(raised.exception),
            "openai (407): the provider answered HTTP 407: no way in for [***], sent as [***], "
            "password [***]",
        )
        for secret in (PASSWORD, BASIC.removeprefix("Basic ")):
            self.assertNotIn(secret, shown)

    def test_a_provider_is_not_built_behind_a_proxy_it_cannot_use(self) -> None:
        refused = {
            f"socks5://{CREDENTIALS}@127.0.0.1:1080": (
                "the environment names a SOCKS proxy, which is not supported"
            ),
            f"ftp://{CREDENTIALS}@127.0.0.1": (
                "a proxy the environment names is neither an http nor an https proxy"
            ),
            f"http://{CREDENTIALS}@127.0.0.1:port": (
                "a proxy the environment names does not read as a URL"
            ),
        }
        for proxy, reason in refused.items():
            with self.subTest(proxy=proxy.partition(":")[0]), environment(ALL_PROXY=proxy):
                with self.assertRaises(ProviderError) as raised:
                    provider("http://llm.example/v1")
                said = str(raised.exception)
                self.assertEqual(said, f"openai: the provider cannot be built: {reason}")
                self.assertNotIn(PASSWORD, f"{said} {raised.exception!r}")


class UnreadTests(unittest.IsolatedAsyncioTestCase):
    async def test_no_key_and_no_header_is_read_from_the_environment(self) -> None:
        endpoint = await Fake().start()
        self.addAsyncCleanup(endpoint.stop)
        endpoint.answer(says("as configured"))
        variables = {
            "OPENAI_API_KEY": "placeholder-of-the-environment",
            "OPENAI_ORG_ID": "organization-of-the-environment",
            "OPENAI_PROJECT_ID": "project-of-the-environment",
        }
        with environment(**variables):
            asked = provider(f"http://127.0.0.1:{endpoint.port}/v1")
            response = await asked.generate(HELLO)
            await asked.close()
        self.assertEqual(response.content, "as configured")
        sent = endpoint.seen[0].headers
        self.assertEqual(sent["authorization"], "Bearer placeholder-not-a-key")
        for name in ("openai-organization", "openai-project"):
            self.assertNotIn(name, sent)

    @unittest.skipUnless(os.environ.get("ROOMKIT_PROVIDERS_NO_NETWORK") == "1", NO_NAMESPACE)
    async def test_no_base_url_is_read_from_the_environment(self) -> None:
        named, proxy = await Fake().start(), await Fake().start()
        self.addAsyncCleanup(named.stop)
        self.addAsyncCleanup(proxy.stop)
        proxy.answer(refuses(502))
        variables = {
            "OPENAI_BASE_URL": f"http://127.0.0.1:{named.port}/v1",
            "HTTPS_PROXY": f"http://127.0.0.1:{proxy.port}",
        }
        with environment(**variables):
            # A provider that names no base URL asks OpenAI's own endpoint,
            # whatever `OPENAI_BASE_URL` names: its request is on its way
            # there when the local proxy refuses its tunnel.
            asked = OpenAI(OpenAIConfig(api_key="placeholder-not-a-key", model="local-model"))
            with self.assertRaises(ProviderError):
                await asked.generate(HELLO)
            await asked.close()
        tunnels = [(seen.method, seen.target) for seen in proxy.seen]
        self.assertEqual(tunnels, [("CONNECT", "api.openai.com:443")])
        self.assertEqual(named.seen, [])


@unittest.skipUnless(OPENSSL, NO_OPENSSL)
class CertificateTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.authority = Authority()
        self.addCleanup(self.authority.close)

    async def serving(self, authority: Authority, *names: str) -> Fake:
        """A server over TLS whose certificate `authority` signed for
        `names`, which answers once."""
        fake = await Fake(authority.serving(*names)).start()
        self.addAsyncCleanup(fake.stop)
        fake.answer(says("trusted"))
        return fake

    async def refused(self, url: str) -> ProviderError:
        asked = provider(url)
        with self.assertRaises(ProviderError) as raised:
            await asked.generate(HELLO)
        await asked.close()
        return raised.exception

    async def test_a_server_is_reached_when_ssl_cert_file_names_its_authority(self) -> None:
        server = await self.serving(self.authority, "127.0.0.1")
        with environment(SSL_CERT_FILE=str(self.authority.pem)):
            asked = provider(f"https://127.0.0.1:{server.port}/v1")
            response = await asked.generate(HELLO)
            await asked.close()
        self.assertEqual(response.content, "trusted")
        self.assertEqual(server.seen[0].target, PATH)

    async def test_a_server_is_refused_when_nothing_names_its_authority(self) -> None:
        server = await self.serving(self.authority, "127.0.0.1")
        other = Authority()
        self.addCleanup(other.close)
        url = f"https://127.0.0.1:{server.port}/v1"
        # No variable, a variable set to nothing, and another authority's
        # file: the system's store decides, or the file does.
        for variables in ({}, {"SSL_CERT_FILE": ""}, {"SSL_CERT_FILE": str(other.pem)}):
            with self.subTest(variables=sorted(variables)), environment(**variables):
                failure = await self.refused(url)
                self.assertEqual(str(failure), "openai: the provider's certificate is not trusted")
                # As RoomKit's provider types a connection that failed.
                self.assertEqual((failure.retryable, failure.status_code), (True, None))
        self.assertEqual(server.seen, [])

    async def test_a_certificate_for_another_name_is_refused(self) -> None:
        server = await self.serving(self.authority, "other.example")
        with environment(SSL_CERT_FILE=str(self.authority.pem)):
            failure = await self.refused(f"https://127.0.0.1:{server.port}/v1")
        self.assertEqual(
            str(failure), "openai: the provider's certificate is not for its host name"
        )
        self.assertEqual(server.seen, [])

    async def test_a_server_is_reached_when_ssl_cert_dir_holds_its_authority(self) -> None:
        server = await self.serving(self.authority, "127.0.0.1")
        url = f"https://127.0.0.1:{server.port}/v1"
        # The directory holds the certificate under a name that is not its
        # hash: OpenSSL does not find it there, and neither does the provider.
        with environment(SSL_CERT_DIR=str(self.authority.directory)):
            failure = await self.refused(url)
        self.assertEqual(str(failure), "openai: the provider's certificate is not trusted")
        with environment(SSL_CERT_DIR=str(self.authority.filed())):
            asked = provider(url)
            response = await asked.generate(HELLO)
            await asked.close()
        self.assertEqual(response.content, "trusted")

    async def test_an_https_endpoint_is_reached_through_a_tunnel_of_the_proxy(self) -> None:
        server = await self.serving(self.authority, "llm.example")
        proxy = await Fake().start()
        self.addAsyncCleanup(proxy.stop)
        proxy.answer(Tunnel(server.port))
        through = f"http://{CREDENTIALS}@127.0.0.1:{proxy.port}"
        with environment(HTTPS_PROXY=through, SSL_CERT_FILE=str(self.authority.pem)):
            asked = provider("https://llm.example/v1")
            response = await asked.generate(HELLO)
            await asked.close()
        self.assertEqual(response.content, "trusted")
        tunnel = proxy.seen[0]
        self.assertEqual((tunnel.method, tunnel.target), ("CONNECT", "llm.example:443"))
        self.assertEqual(tunnel.headers["proxy-authorization"], BASIC)
        carried = server.seen[0]
        self.assertEqual((carried.method, carried.target), ("POST", PATH))
        self.assertNotIn("proxy-authorization", carried.headers)

    def test_a_provider_is_not_built_when_ssl_cert_file_names_no_certificate(self) -> None:
        empty = self.authority.directory / "empty.pem"
        empty.write_text("")
        refused = {
            str(self.authority.directory / "missing.pem"): "cannot be read",
            str(empty): "holds no certificate",
        }
        for file, why in refused.items():
            with self.subTest(why=why), environment(SSL_CERT_FILE=file):
                with self.assertRaises(ProviderError) as raised:
                    provider("http://127.0.0.1:1/v1")
                self.assertEqual(
                    str(raised.exception),
                    f"openai: the provider cannot be built: the file SSL_CERT_FILE names {why}",
                )


if __name__ == "__main__":
    unittest.main()
