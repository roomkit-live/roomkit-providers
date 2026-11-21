"""The providers of language models, one module per vendor.

Each is a RoomKit ``AIProvider``: an AI channel takes it as it takes any
provider. A vendor's module holds its class and its configuration, under
the vendor's own name.

What every provider here shares is written once, in this package: what
RoomKit asks of a provider is translated to the contract the Rust providers
implement, and what they answer to RoomKit's models and exceptions.
"""

__all__: list[str] = []
