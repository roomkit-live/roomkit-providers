"""The providers as RoomKit providers, for a RoomKit agent.

    from roomkit_providers.roomkit import OpenAIAIProvider

This package needs RoomKit, which the distribution's ``roomkit`` extra
installs. The rest of ``roomkit_providers`` does not.
"""

# RoomKit's provider interface is asked for, not the name `roomkit` alone,
# which a directory of that name on the path answers to.
try:
    from roomkit.providers.ai import AIProvider  # noqa: F401
except ImportError as error:
    raise ImportError(
        "roomkit_providers.roomkit needs RoomKit, which is not installed: "
        "install it with the extra, roomkit-providers[roomkit]"
    ) from error

from ._openai import OpenAIAIProvider

__all__ = ["OpenAIAIProvider"]
