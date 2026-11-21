"""What every test of the RoomKit adapter starts with: the helpers of the
package's own tests on the path, and RoomKit installed, without which the
tests are skipped."""

import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

# RoomKit's provider interface, not the name `roomkit` alone: without
# RoomKit, this directory answers to it.
try:
    from roomkit.providers.ai import AIProvider  # noqa: F401
except ImportError:
    raise unittest.SkipTest(
        "RoomKit is not installed: the adapter's tests need roomkit-providers[roomkit]"
    ) from None
