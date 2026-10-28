# Disk Cleanup

Cross-platform disk usage analyzer and safe cleanup assistant for Linux and
Windows. It scans a volume or directory, visualizes what consumes space, finds
large files and exact duplicates, and proposes cleanup actions with an
explanation and a risk level.

## Status

- [x] Rust workspace and scanner
- [x] CLI: volumes, scan, largest, recommend, clean, duplicates
- [x] GUI analyzer (eframe/egui): volumes, treemap, top files/folders, filters
- [x] Custom graphite/teal design system, dark/light/system themes, EN/RU UI
- [x] Safe cleanup: review basket, protected paths, revalidation, trash-only
      apply, JSONL audit log
- [x] Exact duplicates: BLAKE3 pipeline, keeper selection, audited cleanup
- [x] Unit, CLI, cleanup, duplicate, and headless GUI tests
- [x] Linux release build verified
- [ ] Interactive Windows runtime test (the Windows target compiles and passes
      clippy)
- [ ] Games/applications detection
- [ ] Similar photos/videos
- [ ] Packaging

## Features

### Analyze

- Volume picker, recent roots, path entry, and native folder picker.
- Background scan with progress and cancellation; preserves the previous result
  while a new scan is running.
- Treemap with drill-down and breadcrumbs.
- Sortable, virtualized top-files, top-folders, extensions, and scan-issues
  views.
- Name, type, and minimum-size filters with removable chips.
- In-memory cache of the three most recent scans.
- Responsive layout down to 900x600 windows.
- Keyboard shortcuts: `Ctrl+1/2/3` workspaces, `Ctrl+R`/`F5` rescan, `Ctrl+F`
  search, `Enter` open, arrows navigate, `Backspace` up, `Esc` close.

### Safe cleanup

- Built-in common, Linux, and Windows rule packs loaded from TOML.
- Recommendation cards grouped by rule and risk, with safe-select for
  low/medium-risk trash candidates.
- Review basket with explicit confirmation; high-risk items require a separate
  acknowledgement.
- Protected-path policy rejects system roots, profile roots, `System32`,
  `WinSxS`, `WindowsApps`, and similar locations before mutation.
- Item revalidation (`dev`/inode/size/mtime) immediately before trashing.
- Full-precision timestamps, complete directory manifests, linked-ancestor and
  mount-boundary checks; incomplete or changed candidates are refused.
- Background apply progress and Cancel/Escape; Ctrl+C cancels CLI cleanup
  between trash operations while preserving the audit of completed work.
- Deletion is move-to-OS-trash only. There is no permanent-delete fallback.
- Every destructive action writes JSONL intent/result records.
- System/admin cleanup is recommendation-only. Commands are displayed and can
  be copied, never executed.

### Exact duplicates

- Size buckets → bounded prefix hash → full BLAKE3, with progress,
  cancellation, and issue reporting.
- Symlinks and hardlink aliases are excluded; empty files are ignored; files
  changed during hashing are rejected.
- Group cards with minimum-size filtering, keeper selection, per-group and
  total savings, “select rest”, and “all except keepers”.
- Selected copies use the same review basket and audited trash pipeline as
  cleanup.
- Scan evidence is retained through selection; both copies and keepers are
  revalidated and rehashed before cleanup.

Scanning reads directory metadata in bounded parallel batches; duplicate discovery
also hashes candidate files concurrently across size groups. **Settings → Scanning
→ Worker threads** controls both operations: Automatic uses the available CPU
parallelism up to 8 workers; 1 runs sequentially; manual values range from 1 to 32.
Preferences persist and apply to new jobs without restarting. The limit is per
operation, so simultaneous scan/duplicate jobs each have their own pool. Hashing
opens at most one file per worker and uses a 64 KiB read buffer per worker. HDDs
may benefit from fewer workers; speedup depends on storage and the workload.
CLI `scan`, `largest`, and `duplicates` accept `--threads 0|1|…|32` with the same
semantics. For example:

```bash
cargo run -p diskclean-cli -- scan . --threads 4 --json
cargo run -p diskclean-cli -- duplicates . --threads 4 --min-size 1M --json
cargo run -p diskclean-core --example synthetic_scan --release
```

The synthetic benchmark compares 1, 4, and automatic workers using medians of
five warm-cache runs, including worker-pool startup. It covers directory scanning,
prefix-only matching, and full-file hashing; it does not predict cold-cache or
HDD performance.

The UI is available in English and Russian at runtime. The system locale is
used by default when detectable. There is no telemetry or network access.

## Build and run

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

On Linux, build and install a `disk-cleanup` command with:

```bash
./install.sh
disk-cleanup                 # Open the GUI
disk-cleanup ~/Downloads     # Scan a directory
```

The script creates a symlink in `~/.local/bin` to the release GUI in this
checkout. Keep the checkout in place; rerun the script after moving it or
updating the source. An optional first argument selects a different bin
directory. If needed, the script prints the line to add that directory to PATH.
It refuses to overwrite a regular file.

### System directories and sudo on Wayland

Hidden files do not require `sudo`: leave **Skip hidden entries** disabled in
Settings. Some system directories require root permissions to read their
contents.

For a scan with root permissions on a local Wayland desktop, preserve the
session variables so the GUI can connect to the display:

```bash
sudo --preserve-env=WAYLAND_DISPLAY,XDG_RUNTIME_DIR "$HOME/.local/bin/disk-cleanup" /
```

Use the full executable path because `sudo` may not include `~/.local/bin` in
its PATH. Without the display session variables, startup can fail with
`Authorization required, but no authorization protocol specified` or
`XOpenDisplayFailed`. Display access still depends on the compositor and local
sudo policy. The application does not request elevation automatically.

## CLI

```bash
# Read-only analysis
cargo run -p diskclean-cli -- scan . --json
cargo run -p diskclean-cli -- recommend
cargo run -p diskclean-cli -- duplicates . --json --min-size 1M

# Review before mutating anything
cargo run -p diskclean-cli -- clean --dry-run

# Move reviewed cleanup candidates to the OS trash
cargo run -p diskclean-cli -- clean --apply --confirm
```

`clean --apply` requires `--confirm`, excludes high-risk items unless
`--include-high-risk` is passed, and writes an audit log. Use `--rule`,
`--category`, and `--min-size` to narrow the selection. Run `clean --dry-run`
first and inspect the listed paths.

## Verification

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace --target x86_64-pc-windows-gnu
```

The GUI tests use `egui_kittest` and do not open a desktop window.

## Documentation

- [`docs/plan.md`](docs/plan.md) — durable UI/UX and feature delivery plan
- [`docs/ui.md`](docs/ui.md) — design system, layout, interaction, and
  accessibility reference
- [`AGENTS.md`](AGENTS.md) — contributor working agreement, safety invariants,
  dependencies, and status
- [`docs/safety.md`](docs/safety.md) — validation guarantees and OS race limits

## License

GPL-3.0-only. See [LICENSE](LICENSE).
