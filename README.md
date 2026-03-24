# abus

A D-Bus implementation in Rust, built specifically for Tokio.

Most of the existing Rust D-Bus work supports multiple async runtimes. That
flexibility is useful if you need it, but the cost is real: a larger dependency
surface, extra abstraction layers, and a library that doesn't quite fit anywhere.
abus skips all of that. Tokio only, nothing else.

> **Note:** abus is under active development. The roadmap below describes what
> this project is working toward, not what is necessarily implemented today.

## Design

abus is written specifically for Tokio. The connection uses `AsyncFd` directly,
FD passing goes through `sendmsg`/`recvmsg`, and message framing is built on
`tokio_util::codec`. No adapter layers.

The dependency tree is kept deliberately small. The goal is something shallow
enough to actually audit, where each crate is there for a concrete reason.

The API aims to be idiomatic async Rust: async methods, standard
`AsyncRead`/`AsyncWrite` traits, and eventually typed message construction so
you are not manually assembling wire bytes.

## Roadmap

### In progress

Core `abus` library: authentication, connection handling, message marshalling
and unmarshalling, and the full client-side D-Bus protocol. A rough `abusctl`
companion ships alongside to exercise the protocol and validate it against a
real bus.

### Planned

Public API, code generation from D-Bus XML introspection so you don't hand-write
bindings, and a fully featured `abusctl` for inspecting services, calling
methods, and monitoring signals.

### Future

`abusd`: a session and system bus broker daemon. Anything that works with
`dbus-daemon` or `dbus-broker` should work with `abusd` as a drop-in.

### Experimental

Once the core is stable, abus will be a testbed for an alternative IPC protocol.
The idea is full wire compatibility: existing software keeps working, new
software can opt into something better. This is experimental and a long way off.

## License

This project is licensed under the
[Apache-2.0 License](http://www.apache.org/licenses/LICENSE-2.0). For more
information, please see the [LICENSE](LICENSE) file.
