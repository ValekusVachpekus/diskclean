# AGENTS.md

This file is the working agreement for agents contributing to Disk Cleanup.
Keep it current after every meaningful milestone.

The durable UI/UX and feature delivery plan lives in
[`docs/plan.md`](docs/plan.md). Read it first after a context reset. This file
summarizes the non-negotiable rules; the plan tracks detailed milestones and
progress.

## Project

Cross-platform (Linux + Windows) disk usage analyzer and safe cleanup assistant.
It scans a volume or directory, visualizes what consumes space, finds large
files/folders and duplicates, detects games/applications, and proposes cleanup
actions with an explanation and a risk level.

## Fixed product decisions

- Language: Rust 2024.
- GUI: `eframe` / `egui` (phase 1).
- UI languages: English and Russian, switchable at runtime; system locale is
  the default when detectable. All user-facing text and rule explanations are
  localized.
- UI design: custom design system (graphite surfaces, teal accent, dark/light/
  system themes), Inter for text, a monospaced face for numeric columns, and
  Phosphor icons. Do not fall back to stock egui styling for new screens.
- License: GPL-3.0-only.
- Platforms: Linux first, Windows supported from the start of the design.
- Deletion: move to the OS trash only, after explicit confirmation.
- No UAC/pkexec helper in the MVP. System cleanup is recommendation-only and
  displays commands the user may run themselves.
- Similar photos/videos: later phases, using `czkawka_core` and a system
  `ffmpeg`. Video support must be feature-gated and optional.
- No telemetry, no network access by default.

## Architecture

```
crates/diskclean-core/      scanner, tree model, duplicate engine, reports
crates/diskclean-platform/  OS-specific metadata, volumes, trash, apps, packages
crates/diskclean-rules/     rule schema/engine and built-in rule packs (phase 2)
crates/diskclean-cleanup/   review basket, protected paths, revalidation, audit (planned phase 2)
crates/diskclean-cli/       clap CLI and JSON output
apps/diskclean-gui/         eframe/egui application (phase 1)
rules/                      common/linux/windows TOML rule packs (phase 2)
docs/plan.md                durable UI/UX and feature delivery plan
```

`diskclean-cleanup` does not exist yet; create it in phase 2. It will own all
filesystem mutation, the `TrashBackend` trait, protected-path policy,
revalidation, and the JSONL audit log. `diskclean-rules` stays pure and never
mutates the filesystem.

The core must not depend on the GUI. The platform crate exposes low-level OS
facts; the core maps them into its own tree model. Avoid circular dependencies.

## Safety invariants

These are non-negotiable:

1. Scanning is read-only.
2. Nothing is deleted without explicit user confirmation.
3. Default action is moving to the OS trash, never permanent deletion.
4. Protected paths must be rejected before any filesystem mutation. This
   includes `/`, `/boot`, `/etc`, `/usr`, `/var/lib`, `C:\`, `C:\Windows`,
   `C:\Program Files*`, the user profile root, `System32`, `WinSxS`, and
   `WindowsApps`.
5. Never follow symlinks, junctions, or reparse points by default.
6. Never cross filesystem/mount boundaries unless the user explicitly asks.
7. Re-check `dev/inode/size/mtime` immediately before a destructive action.
8. Never construct shell commands by concatenating user-controlled paths.
9. No privileged code in the MVP. If elevation is added later, it must be a
   separate allowlisted helper, never the GUI process.
10. Every destructive action must be written to a local audit log.

## Build and test commands

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p diskclean-core --example synthetic_scan --release
cargo run -p diskclean-cli -- volumes
cargo run -p diskclean-cli -- scan . --json
cargo run -p diskclean-cli -- scan / --json --top 50 --exclude /proc --exclude /sys
cargo run -p diskclean-gui -- .
cargo run -p diskclean-gui -- /path/to/directory
```

CI additionally builds on `windows-latest`. Linux jobs install the X11,
Wayland, xkbcommon, and Mesa development packages needed by `eframe`.

## Coding conventions

- Library code must not `unwrap()` or `expect()` on user/filesystem input.
  Use `Result` and collect non-fatal scan problems as `ScanIssue`.
- Prefer `thiserror` for typed library errors and `anyhow` at the CLI boundary.
- Keep OS-specific code behind `cfg(target_os = ...)` and in
  `diskclean-platform`; document any `unsafe` block and keep it tiny.
- Use `Path`/`PathBuf`/`OsString` internally. Only convert to `String` at the
  JSON/display boundary, and use lossy conversion there.
- Add unit tests for every tree/size/hardlink/symlink behavior.
- Keep dependencies permissive and GPL-compatible. Approved now:
  `czkawka_core` (MIT), `egui`/`eframe`/`egui_extras`/`egui_kittest`
  (MIT/Apache-2.0), `rfd` (MIT), `trash` (MIT), Phosphor icons (MIT/Apache-2.0),
  Inter and JetBrains Mono (OFL-1.1), `sys-locale` (MIT/Apache-2.0).
  Approved for the listed future phases when the feature actually lands:
  `blake3` (CC0-1.0/Apache-2.0), `toml` and `globset` (MIT/Apache-2.0),
  `serde_json` (MIT/Apache-2.0), `winreg` (MIT/Apache-2.0), `image`
  (MIT/Apache-2.0). `rusqlite` (MIT) may be used only for Lutris discovery and
  only behind a feature/`cfg`; prefer parsing configs if practical.
  Do not copy AGPL code from Squirreldisk. QDirStat is GPL-2.0 and its code is
  not GPL-3.0 compatible; use ideas only.
- Do not add `unsafe` to the core crate. Platform wrappers may use it only when
  the standard library cannot expose the required metadata.

## Current status

Phase 0 foundation complete:

- [x] Cargo workspace and GPL-3.0 license
- [x] AGENTS.md
- [x] `diskclean-platform`: Linux metadata + volume enumeration
- [x] `diskclean-core`: tree, scanner, progress, JSON report
- [x] `diskclean-cli`: `scan`, `largest`, `volumes`
- [x] Unit tests and a synthetic benchmark
- [x] CI workflow for Linux and Windows (Windows target cross-checked locally;
      hosted CI runs after the repository is pushed)

Phase 1 GUI analyzer implemented:

- [x] Volume picker, recent roots, path entry, and native folder picker
- [x] Background scan with progress and cancellation
- [x] Treemap with drill-down and breadcrumbs
- [x] Top files, top folders, extensions, and scan issues
- [x] Name, type, and minimum-size filters
- [x] In-memory cache of the three most recent scans
- [x] Responsive panels for narrow tiled windows
- [x] Linux smoke test; Windows target compiles and passes clippy
- [x] Linux release binaries built locally
- [ ] Interactive Windows test and automated GUI tests

UI/UX redesign U0 is in progress (see `docs/plan.md`). The current GUI is still
the phase-1 implementation until the new shell lands; do not mark U0 or U1 done
early.

Later phases are not started.

## Roadmap

0. Foundation: scanner, tree model, CLI, tests, CI.
1. GUI analyzer: volumes, treemap, top files/folders, filters, cache.
2. UI/UX redesign: design system, shell, navigation, i18n, persisted settings,
   sortable virtualized tables, responsive inspector. Detailed milestones U0–U3
   are in `docs/plan.md`.
3. Safe cleanup: temp/caches/trash/old downloads, review basket, audit log.
4. Exact duplicates: size -> prefix hash -> full BLAKE3, hardlink handling.
5. Apps/games: Steam/Epic/GOG/Heroic/Lutris, Windows registry, Linux packages.
6. Similar photos via `czkawka_core`.
7. Similar videos via `czkawka_core` + system `ffmpeg`.
8. System command recommendations and packaging polish.

## Maintenance rule for this file

Update `AGENTS.md` when any of these change:

- fixed product decisions;
- crate layout or commands;
- safety invariants;
- phase status/roadmap;
- approved/unapproved dependencies or licenses.

Update `docs/plan.md` at least at every milestone, and check off items only
when their acceptance criteria are implemented and tested. Keep both files
honest after a context reset.
