"""The adapter's public API against its committed snapshot, as the
package's own is checked in ``tests/python/test_public_api.py``."""

import unittest

import adapter_tests
import api_signatures
from api_signatures import Package

ADAPTER = Package("roomkit_providers.roomkit")


class SnapshotTests(unittest.TestCase):
    def test_the_snapshot_holds_the_adapter_s_whole_public_surface(self):
        drift = api_signatures.drift(adapter_tests.HERE, ADAPTER)
        self.assertEqual(drift, [], "run make api-snapshot and commit the rewritten files")

    def test_the_adapter_is_roomkit_s_provider(self):
        signatures = api_signatures.build(ADAPTER)
        provider = signatures["roomkit_providers.roomkit.OpenAIAIProvider"]
        self.assertEqual(provider["__bases__"], ["bases", "roomkit.providers.ai.base.AIProvider"])
        self.assertEqual(provider["__init__"], ["def", "config"])


if __name__ == "__main__":
    unittest.main()
