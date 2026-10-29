# Dependency notes

This records the Lutris dependency milestone, not a complete cargo-deny audit.
The release license/notice gate remains in the packaging plan.

## Linux Lutris inventory

Platform's default `lutris` feature enables optional Linux-only `rusqlite`
0.41.1 (MIT) and the existing `tempfile` workspace dependency (MIT/Apache-2.0).
The approved SQLite dependency is used only to inspect Lutris metadata snapshots;
no GUI, rules or cleanup code opens databases. YAML-only parsing cannot supply
the authoritative installed flag and installation directory for all Lutris games.

The `bundled` feature builds SQLite via `libsqlite3-sys` 0.39.1 (MIT wrapper;
SQLite itself is public domain). Users need no SQLite executable or system
SQLite library. `hooks` supports query cancellation; `limits` bounds values and
query resources. Dynamic extension loading is not enabled.

New Linux runtime transitive packages are `hashlink` 0.12.2,
`fallible-iterator` 0.3.0 and `fallible-streaming-iterator` 0.1.9
(MIT OR Apache-2.0), alongside previously used dependencies. `vcpkg` 0.2.15
(MIT OR Apache-2.0) is a build dependency. Target-specific packages in Cargo.lock are not
necessarily compiled into the Linux/Windows binaries. Windows builds do not
compile the Lutris SQLite reader. Linux-only fixture tests in core/CLI use the
same SQLite dependency to generate real test databases.

Metadata snapshots use a private temporary directory; original DB/WAL/SHM files
are not opened by SQLite or modified. See [application scope](applications.md).

To check the platform without SQLite support:

```bash
cargo check -p diskclean-platform --no-default-features --locked
```

Sources: [rusqlite](https://github.com/rusqlite/rusqlite),
[SQLite public-domain dedication](https://www.sqlite.org/copyright.html).
