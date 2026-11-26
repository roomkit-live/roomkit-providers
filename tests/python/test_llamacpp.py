"""The managed server's public Python lifecycle, with no model or external service."""

from __future__ import annotations

import os
import sys
import tempfile
import unittest
from pathlib import Path

from roomkit.providers.llamacpp import LlamaCppConfig as ReferenceConfig
from roomkit_providers.llm.llamacpp import LlamaCpp, LlamaCppConfig

SERVER = """
import json, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        data = {"status": "ok"} if self.path == "/health" else {"data": [{"id": "local"}]}
        reply = json.dumps(data).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(reply)))
        self.end_headers()
        self.wfile.write(reply)

args = sys.argv[1:]
Path(args[args.index("--record") + 1]).write_text(json.dumps(args))
HTTPServer(("127.0.0.1", int(args[args.index("--port") + 1])), Handler).serve_forever()
"""


class ManagedTests(unittest.IsolatedAsyncioTestCase):
    @unittest.skipUnless(os.name == "posix", "the stand-in executable uses a shebang")
    async def test_load_once_and_list_the_served_model(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / "server"
            binary.write_text(f"#!{sys.executable}\n{SERVER}")
            binary.chmod(0o700)
            gguf = root / "example.gguf"
            gguf.touch()
            record = root / "launch"
            config = ReferenceConfig(
                model=str(gguf), binary=str(binary), extra_args=["--record", str(record)]
            )
            provider = LlamaCpp(config)
            self.assertTrue(provider.base_url.startswith("http://127.0.0.1:"))
            self.assertEqual(provider.available_models(), [])
            self.assertFalse(record.exists())
            try:
                await provider.start()
                self.assertIn(str(gguf), record.read_text())
                self.assertEqual([m.id for m in await provider.list_models()], ["local"])
            finally:
                await provider.close()

    def test_configuration_matches_the_reference(self) -> None:
        self.assertEqual(
            LlamaCppConfig(model="local").model_dump(), ReferenceConfig(model="local").model_dump()
        )
        for values in ({"model": " "}, {"model": "local", "variant": "missing"}):
            with self.assertRaises(ValueError):
                LlamaCppConfig(**values)
