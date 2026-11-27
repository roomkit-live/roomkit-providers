"""What every provider's configuration shares.

A provider's own module states the members of its configuration, and
nothing of what follows: how a configuration shows itself, what it refuses
when it is built, the switches its vendor decides from the model, and the
refusal of a forked child. They are written here once, so that no provider
can leave one out.
"""

from __future__ import annotations

import json
from collections.abc import Iterator, Mapping
from typing import Any, ClassVar, get_args
from urllib.parse import urlsplit

from pydantic import BaseModel, ConfigDict, SecretStr, model_validator

from .. import _native, _process
from ._failure import failing_as_roomkit

MASK = "**********"


def shown_url(url: str) -> str:
    """`url` as it is shown: its scheme, its host and its port. A user and a
    password, a path and a query may each hold a credential, and a URL that
    does not read is not shown at all."""
    try:
        read = urlsplit(url)
        host, port = read.hostname, read.port
    except ValueError:
        return MASK
    if not read.scheme or host is None:
        return MASK
    host = f"[{host}]" if ":" in host else host
    return f"{read.scheme}://{host}" + ("" if port is None else f":{port}")


class Config(BaseModel):
    """A provider's configuration: a pydantic model of its members.

    A provider's module derives its configuration from this class and
    states its members, its key among them where it has one: ``api_key``, a
    ``SecretStr``, which may be optional. A provider that takes no key
    states no such member. The class statement says what follows beside
    the members::

        class VendorConfig(
            Config,
            vendor="vendor",
            stands_for=RoomKitVendorConfig,
            decided=("a_switch",),
            rewritten=("an_endpoint",),
            urls=("an_endpoint",),
        ):

    ``vendor`` is the name the native module holds the provider under;
    ``stands_for``, RoomKit's configuration of the same provider, which the
    provider takes in place of this one; ``decided``, the switches the
    vendor decides from the model on its own endpoint; ``rewritten``, the
    members the provider reads its own way, which take what its reading
    makes of them (an endpoint reduced to the one it asks); and ``urls``,
    the members beside ``base_url`` that hold a URL. The last three may
    be left out.

    What every configuration then does:

    - ``repr`` never shows a key, which is a ``SecretStr`` or the class is
      refused, the value of any member of a mapping (a header, a member of
      a request's body), nor more of ``base_url`` or of a member of
      ``urls`` than its scheme, its host and its port;
    - a refusal says what is refused and never shows the value given,
      which may be a secret;
    - a ``base_url`` Python's URL reader refuses is refused when the
      configuration is built, as RoomKit's configurations refuse it;
    - each switch of ``_decided`` left unset is decided when the
      configuration is built, by the provider's own rule, in native code;
    - each member of ``_rewritten`` takes, when the configuration is
      built, what the provider's own reading makes of it, in native code;
      a value the reading refuses is refused;
    - a process forked after its parent's first call is refused before
      anything is read.
    """

    model_config = ConfigDict(hide_input_in_errors=True)

    _vendor: ClassVar[str]
    _stands_for: ClassVar[type[BaseModel] | None] = None
    _decided: ClassVar[frozenset[str]] = frozenset()
    _rewritten: ClassVar[frozenset[str]] = frozenset()
    _urls: ClassVar[frozenset[str]] = frozenset({"base_url"})

    def __init__(self, /, **data: Any) -> None:
        if _process.forked():
            # The refusal's traceback shows this frame: it holds nothing of
            # what was given.
            data = {}
            _process.enter()
        super().__init__(**data)

    def __init_subclass__(
        cls,
        *,
        vendor: str | None = None,
        stands_for: type[BaseModel] | None = None,
        decided: tuple[str, ...] = (),
        rewritten: tuple[str, ...] = (),
        urls: tuple[str, ...] = (),
        **kwargs: Any,
    ) -> None:
        # What the class statement says is read once the members are known,
        # below.
        super().__init_subclass__(**kwargs)

    @classmethod
    def __pydantic_init_subclass__(
        cls,
        *,
        vendor: str | None = None,
        stands_for: type[BaseModel] | None = None,
        decided: tuple[str, ...] = (),
        rewritten: tuple[str, ...] = (),
        urls: tuple[str, ...] = (),
        **kwargs: Any,
    ) -> None:
        super().__pydantic_init_subclass__(**kwargs)
        key = cls.model_fields.get("api_key")
        # A key masks itself wherever a configuration shows: stated, or left
        # out by one that states none.
        if key is not None and key.annotation is not SecretStr:
            stated = set(get_args(key.annotation))
            if stated != {SecretStr, type(None)}:
                raise TypeError("a configuration holds its key as a SecretStr (api_key)")
        if vendor is None:
            # A configuration derived from a provider's is that provider's.
            if not hasattr(cls, "_vendor"):
                raise TypeError("a configuration names the provider it configures (vendor=...)")
            return
        unknown = sorted(set(decided) - set(cls.model_fields))
        if unknown:
            raise TypeError(f"a configuration decides only its own members, not {unknown}")
        unknown = sorted((set(rewritten) | set(urls)) - set(cls.model_fields))
        if unknown:
            raise TypeError(f"a configuration names only its own members, not {unknown}")
        cls._vendor, cls._stands_for, cls._decided = vendor, stands_for, frozenset(decided)
        cls._rewritten, cls._urls = frozenset(rewritten), frozenset({"base_url", *urls})

    def model_post_init(self, context: Any, /) -> None:
        """Refuse a base URL that does not read, then decide the switches
        the vendor decides from the model, each unless it was stated."""
        if isinstance(getattr(self, "base_url", None), str):
            try:
                urlsplit(self.base_url.strip())
            except ValueError:
                # The reader's own words quote the URL, which may hold a
                # credential.
                raise ValueError("base_url does not read as a URL") from None
        unset = type(self)._decided - self.model_fields_set
        if unset:
            self._decide(unset)

    @model_validator(mode="after")
    def _rewrite(self) -> Config:
        """Take from the native module what the provider's own reading makes
        of the members of `_rewritten`. A value it refuses raises the
        reader's ValueError, which never repeats the value."""
        rewritten = type(self)._rewritten
        if not rewritten:
            return self
        _process.enter()
        asked = self.model_dump(mode="json")
        if "api_key" in asked:
            # The key is no member the reading rewrites, and is not sent.
            asked["api_key"] = ""
        read = json.loads(_native.config_form(type(self)._vendor, json.dumps(asked)))
        for name in sorted(rewritten):
            if getattr(self, name) != read[name]:
                setattr(self, name, read[name])
        return self

    def _decide(self, unset: frozenset[str]) -> None:
        """Take from the native module what the vendor decides of `unset`,
        the switches left unset, for this model behind this base URL."""
        _process.enter()
        members = type(self).model_fields
        asked = {name: getattr(self, name) for name in ("model", "base_url") if name in members}
        if "api_key" in members:
            # The key decides nothing, and is not sent to decide.
            asked["api_key"] = ""
        with failing_as_roomkit():
            decided = json.loads(_native.config_form(type(self)._vendor, json.dumps(asked)))
        for name in sorted(unset):
            if getattr(self, name) != decided[name]:
                setattr(self, name, decided[name])

    def __repr_args__(self) -> Iterator[tuple[str | None, Any]]:
        for name, value in super().__repr_args__():
            if isinstance(value, Mapping):
                value = dict.fromkeys(value, MASK)
            elif name in type(self)._urls and isinstance(value, str):
                value = shown_url(value)
            yield name, value


class Veiled:
    """A configuration held so that no frame shows it: what a traceback
    writes of a frame's locals is each one's ``repr``, and a configuration's
    may show a header's value or a URL's password.

    Nothing here binds a secret to a name where something can still raise:
    the key leaves as the object that masks it, which the native module
    opens, and the JSON form is handed on as it is written.
    """

    __slots__ = ("_held", "_own")

    def __init__(self, held: Any, own: type[Config]) -> None:
        self._held = held
        self._own = own

    def __repr__(self) -> str:
        return "<a configuration>"

    def taken(self) -> bool:
        """Whether the configuration is the provider's own, or RoomKit's of
        the same provider: one derived from either is another provider's."""
        return type(self._held) in (self._own, self._own._stands_for)

    def held(self) -> Any:
        return self._held

    def own(self) -> Config:
        """The configuration as the provider's own, whose ``repr`` shows no
        secret: RoomKit's is copied member by member, unchecked, since
        RoomKit checked it."""
        if type(self._held) is self._own:
            return self._held
        members = self._own.model_fields
        return self._own.model_construct(
            _fields_set=set(members),
            **{name: getattr(self._held, name) for name in members},
        )

    def native(self) -> _native._Provider:
        """The native provider of the configuration, with the key it states,
        if it states one."""
        own = self.own()
        key = getattr(own, "api_key", None)
        return _native.provider(self._own._vendor, own.model_dump_json(), key)
