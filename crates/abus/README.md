# abus

A D-Bus implementation in Rust, built specifically for Tokio.

Most of the existing Rust D-Bus work supports multiple async runtimes. That
flexibility is useful if you need it, but the cost is real: a larger dependency
surface, extra abstraction layers, and a library that doesn't quite fit
anywhere. abus skips all of that. Tokio only, nothing else.

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

## Usage

```toml
[dependencies]
abus = "0.0.2"
```

## License

Licensed under the [Apache-2.0 License](../../LICENSE).
