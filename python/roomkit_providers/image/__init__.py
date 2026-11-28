"""The image providers (RFC §25), one module per vendor.

Each is a RoomKit ``ImageProvider``: it is given to RoomKit as RoomKit's own
is. A vendor's module holds its class and its configuration, under the
vendor's own name.

What every image provider here shares is written once, in this package: a
request crosses to the contract the Rust providers implement, and what they
answer comes back as RoomKit's models and exceptions, each attempt told to
the host's progress callback.
"""

__all__: list[str] = []
