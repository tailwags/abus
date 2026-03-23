# abus

A D-Bus implementation in Rust, built specifically for Tokio.

Most of the existing Rust D-Bus work supports multiple async runtimes. That
flexibility is useful if you need it, but it comes with a real cost: a larger
dependency surface, extra abstraction layers, and a library that doesn't quite
fit anywhere. abus skips all of that. Tokio only, nothing else.

> **Note:** abus is under active development. The roadmap below describes what
> this project is working toward, not what is necessarily implemented today.

## Why

The difference is most obvious in the dependency trees. Here are the non-dev,
default-feature trees for both:

<details>
<summary><strong>abus 0.0.1</strong></summary>

```
abus v0.0.1
├── bitflags v2.11.0
├── bytes v1.11.1
├── getrandom v0.4.2
│   ├── cfg-if v1.0.4
│   └── libc v0.2.183
├── itoa v1.0.18
├── pin-project-lite v0.2.17
├── rustix v1.1.4
│   ├── bitflags v2.11.0
│   └── linux-raw-sys v0.12.1
├── serde_core v1.0.228
├── tokio v1.50.0
│   ├── bytes v1.11.1
│   ├── libc v0.2.183
│   ├── mio v1.1.1
│   │   └── libc v0.2.183
│   ├── pin-project-lite v0.2.17
│   └── socket2 v0.6.3
│       └── libc v0.2.183
├── tokio-stream v0.1.18
│   ├── futures-core v0.3.32
│   ├── pin-project-lite v0.2.17
│   └── tokio v1.50.0 (*)
└── tokio-util v0.7.18
    ├── bytes v1.11.1
    ├── futures-core v0.3.32
    ├── futures-sink v0.3.32
    ├── pin-project-lite v0.2.17
    └── tokio v1.50.0 (*)
```

</details>

<details>
<summary><strong>zbus 5.14.0</strong></summary>

```
zbus v5.14.0
├── async-broadcast v0.7.2
│   ├── event-listener v5.4.1
│   │   ├── concurrent-queue v2.5.0
│   │   │   └── crossbeam-utils v0.8.21
│   │   ├── parking v2.2.1
│   │   └── pin-project-lite v0.2.16
│   ├── event-listener-strategy v0.5.4
│   │   ├── event-listener v5.4.1 (*)
│   │   └── pin-project-lite v0.2.16
│   ├── futures-core v0.3.32
│   └── pin-project-lite v0.2.16
├── async-executor v1.14.0
│   ├── async-task v4.7.1
│   ├── concurrent-queue v2.5.0 (*)
│   ├── fastrand v2.3.0
│   ├── futures-lite v2.6.1
│   │   ├── fastrand v2.3.0
│   │   ├── futures-core v0.3.32
│   │   ├── futures-io v0.3.32
│   │   ├── parking v2.2.1
│   │   └── pin-project-lite v0.2.16
│   ├── pin-project-lite v0.2.16
│   └── slab v0.4.11
├── async-io v2.6.0
│   ├── cfg-if v1.0.4
│   ├── concurrent-queue v2.5.0 (*)
│   ├── futures-io v0.3.32
│   ├── futures-lite v2.6.1 (*)
│   ├── parking v2.2.1
│   ├── polling v3.11.0
│   │   ├── cfg-if v1.0.4
│   │   └── rustix v1.1.4
│   │       ├── bitflags v2.10.0
│   │       └── linux-raw-sys v0.12.1
│   ├── rustix v1.1.4 (*)
│   └── slab v0.4.11
├── async-lock v3.4.2
│   ├── event-listener v5.4.1 (*)
│   ├── event-listener-strategy v0.5.4 (*)
│   └── pin-project-lite v0.2.16
├── async-process v2.5.0
│   ├── async-channel v2.5.0
│   │   ├── concurrent-queue v2.5.0 (*)
│   │   ├── event-listener-strategy v0.5.4 (*)
│   │   ├── futures-core v0.3.32
│   │   └── pin-project-lite v0.2.16
│   ├── async-io v2.6.0 (*)
│   ├── async-lock v3.4.2 (*)
│   ├── async-signal v0.2.13
│   │   ├── async-io v2.6.0 (*)
│   │   ├── cfg-if v1.0.4
│   │   ├── futures-core v0.3.32
│   │   ├── futures-io v0.3.32
│   │   ├── rustix v1.1.4 (*)
│   │   └── signal-hook-registry v1.4.7
│   │       └── libc v0.2.183
│   ├── async-task v4.7.1
│   ├── cfg-if v1.0.4
│   ├── event-listener v5.4.1 (*)
│   ├── futures-lite v2.6.1 (*)
│   └── rustix v1.1.4 (*)
├── async-recursion v1.1.1 (proc-macro)
│   ├── proc-macro2 v1.0.106
│   │   └── unicode-ident v1.0.22
│   ├── quote v1.0.45
│   │   └── proc-macro2 v1.0.106 (*)
│   └── syn v2.0.117
│       ├── proc-macro2 v1.0.106 (*)
│       ├── quote v1.0.45 (*)
│       └── unicode-ident v1.0.22
├── async-task v4.7.1
├── async-trait v0.1.89 (proc-macro)
│   ├── proc-macro2 v1.0.106 (*)
│   ├── quote v1.0.45 (*)
│   └── syn v2.0.117 (*)
├── blocking v1.6.2
│   ├── async-channel v2.5.0 (*)
│   ├── async-task v4.7.1
│   ├── futures-io v0.3.32
│   ├── futures-lite v2.6.1 (*)
│   └── piper v0.2.4
│       ├── atomic-waker v1.1.2
│       ├── fastrand v2.3.0
│       └── futures-io v0.3.32
├── enumflags2 v0.7.12
│   ├── enumflags2_derive v0.7.12 (proc-macro)
│   │   ├── proc-macro2 v1.0.106 (*)
│   │   ├── quote v1.0.45 (*)
│   │   └── syn v2.0.117 (*)
│   └── serde v1.0.228
│       ├── serde_core v1.0.228
│       └── serde_derive v1.0.228 (proc-macro)
│           ├── proc-macro2 v1.0.106 (*)
│           ├── quote v1.0.45 (*)
│           └── syn v2.0.117 (*)
├── event-listener v5.4.1 (*)
├── futures-core v0.3.32
├── futures-lite v2.6.1 (*)
├── hex v0.4.3
├── libc v0.2.183
├── ordered-stream v0.2.0
│   ├── futures-core v0.3.32
│   └── pin-project-lite v0.2.16
├── rustix v1.1.4 (*)
├── serde v1.0.228 (*)
├── serde_repr v0.1.20 (proc-macro)
│   ├── proc-macro2 v1.0.106 (*)
│   ├── quote v1.0.45 (*)
│   └── syn v2.0.117 (*)
├── tracing v0.1.44
│   ├── pin-project-lite v0.2.16
│   ├── tracing-attributes v0.1.31 (proc-macro)
│   │   ├── proc-macro2 v1.0.106 (*)
│   │   ├── quote v1.0.45 (*)
│   │   └── syn v2.0.117 (*)
│   └── tracing-core v0.1.36
│       └── once_cell v1.21.3
├── uuid v1.22.0
│   └── serde_core v1.0.228
├── winnow v1.0.0
├── zbus_macros v5.14.0 (proc-macro)
│   ├── proc-macro-crate v3.5.0
│   │   └── toml_edit v0.25.3+spec-1.1.0
│   │       ├── indexmap v2.12.1
│   │       │   ├── equivalent v1.0.2
│   │       │   └── hashbrown v0.16.1
│   │       ├── toml_datetime v1.0.0+spec-1.1.0
│   │       ├── toml_parser v1.0.9+spec-1.1.0
│   │       │   └── winnow v0.7.15
│   │       └── winnow v0.7.15
│   ├── proc-macro2 v1.0.106 (*)
│   ├── quote v1.0.45 (*)
│   ├── syn v2.0.117 (*)
│   ├── zbus_names v4.3.1
│   │   ├── serde v1.0.228 (*)
│   │   ├── winnow v1.0.0
│   │   └── zvariant v5.10.0
│   │       ├── endi v1.1.1
│   │       ├── enumflags2 v0.7.12 (*)
│   │       ├── serde v1.0.228 (*)
│   │       ├── winnow v1.0.0
│   │       ├── zvariant_derive v5.10.0 (proc-macro)
│   │       │   ├── proc-macro-crate v3.5.0 (*)
│   │       │   ├── proc-macro2 v1.0.106 (*)
│   │       │   ├── quote v1.0.45 (*)
│   │       │   ├── syn v2.0.117 (*)
│   │       │   └── zvariant_utils v3.3.0
│   │       │       ├── proc-macro2 v1.0.106 (*)
│   │       │       ├── quote v1.0.45 (*)
│   │       │       ├── serde v1.0.228 (*)
│   │       │       ├── syn v2.0.117 (*)
│   │       │       └── winnow v1.0.0
│   │       └── zvariant_utils v3.3.0 (*)
│   ├── zvariant v5.10.0 (*)
│   └── zvariant_utils v3.3.0 (*)
├── zbus_names v4.3.1 (*)
└── zvariant v5.10.0 (*)
```

</details>

zbus supports multiple async runtimes. That's a reasonable thing to want, and
the dependency count is what it costs. abus doesn't need the flexibility, so it
skips it: Tokio only, written from scratch.

The same thinking applies to third-party crates generally. A few examples:

- Hex encoding shows up in exactly two places, so there's no `hex` crate. A
  dozen lines of code don't justify pulling in a library.
- D-Bus UUID is explicitly not standard UUID v4, so instead of reaching for the
  `uuid` crate, abus implements it directly. The approach is adapted from
  [nanoid](https://github.com/ai/nanoid): 16 random bytes from `getrandom`,
  hex-encoded to 32 characters as the spec requires.

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
