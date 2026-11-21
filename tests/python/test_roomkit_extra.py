"""The package without RoomKit: it imports and works, and its RoomKit
adapter says what it needs."""

from __future__ import annotations

import importlib
import unittest
from importlib import metadata

import roomkit_providers
import roomkit_providers.openai


def roomkit_is_installed() -> bool:
    """Whether the environment has RoomKit's distribution: the name `roomkit`
    alone is also the directory of the adapter's tests, beside this file."""
    try:
        metadata.distribution("roomkit")
    except metadata.PackageNotFoundError:
        return False
    return True


class WithoutRoomKitTests(unittest.TestCase):
    def test_the_adapter_says_what_it_needs_where_roomkit_is_not_installed(self) -> None:
        if roomkit_is_installed():
            self.skipTest("RoomKit is installed: tests/python/roomkit holds the adapter's tests")
        with self.assertRaises(ImportError) as refused:
            importlib.import_module("roomkit_providers.roomkit")
        self.assertIn("roomkit-providers[roomkit]", str(refused.exception))
        # The package itself needs none of it.
        self.assertTrue(roomkit_providers.openai.OpenAIAIProvider.available_models())
        self.assertNotIn("roomkit", vars(roomkit_providers))


if __name__ == "__main__":
    unittest.main()
