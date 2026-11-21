"""RoomKit's providers, implemented in Rust.

A provider here is a RoomKit provider: it is given to RoomKit as RoomKit's
own is. The providers are filed by kind, then by vendor, one module each,
and each module holds the provider's class and its configuration:

    roomkit_providers.llm        the providers of language models
"""

__version__ = "0.1.0a1"

__all__ = ["__version__"]
