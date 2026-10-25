# Disk Cleanup

Cross-platform disk usage analyzer and safe cleanup assistant for Linux and
Windows.

The project is in an early phase: the scanner, tree model, CLI, and the first
GUI analyzer exist. Safe cleanup recommendations are next.

## Status

- [x] Rust workspace
- [x] Linux filesystem scanner and CLI
- [x] Unit/integration tests and a synthetic benchmark
- [x] GUI analyzer (eframe/egui): volumes, treemap, top files/folders, filters
- [x] Linux release build verified
- [ ] Safe cleanup recommendations
- [ ] Exact duplicates
- [ ] Games/applications detection
- [ ] Similar photos/videos

## Build

```bash
cargo build --release
cargo run -p diskclean-cli -- volumes
cargo run -p diskclean-cli -- scan ~ --json
cargo run -p diskclean-gui -- .
cargo run -p diskclean-gui -- ~/Downloads
```

The GUI accepts an optional path; when provided it starts scanning immediately.
Release binaries are written to `target/release/diskclean` and
`target/release/diskclean-gui`.

## License

GPL-3.0-only. See [LICENSE](LICENSE).
