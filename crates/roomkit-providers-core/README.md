# roomkit-providers-core

Runtime-independent contracts for model providers: what a provider is asked,
what it answers, the events of a streamed answer, typed failures and model
catalogues.

The crate has no I/O and no async runtime, and depends on neither Python nor
RoomKit. Provider implementations own their HTTP; hosts own their
conversations.

**Early development.** The crate defines no item yet. See the
[repository](https://github.com/roomkit-live/roomkit-providers) for the state
of the project.

## License

MIT
