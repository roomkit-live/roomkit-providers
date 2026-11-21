"""Which process may enter the native module, and what it owes it on leaving.

The native module runs its calls on threads of its own. A child forked after
its parent's first call has none of them: nothing there would ever complete
a call, and a lock one of those threads held at the fork stays held in the
child for good. Such a child is refused here, in Python, before it enters
the native module at all.
"""

from __future__ import annotations

import atexit
import os

from . import _native

_FORKED = (
    "a provider cannot be used in a process forked after its parent's first call: "
    "make the first call after forking, or use the spawn or forkserver start method"
)

# The process whose first call started the native module's threads.
_started_in: int | None = None


def forked() -> bool:
    """Whether this process was forked after its parent's first call."""
    return _started_in is not None and _started_in != os.getpid()


def enter() -> None:
    """Called before an entry into the native module: a forked child is
    refused without entering it."""
    if forked():
        raise RuntimeError(_FORKED)


def start() -> None:
    """Called before a call, which may start the native module's threads."""
    global _started_in
    enter()
    if _started_in is None:
        _started_in = os.getpid()


def _leave() -> None:
    """At the interpreter's exit, the calls still under way are stopped, and
    the exit waits until each has ended. A forked child has none: it leaves
    without entering the native module."""
    if not forked():
        _native._shutdown()


atexit.register(_leave)
