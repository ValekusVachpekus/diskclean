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
- GUI: `eframe` / `egui`.
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
crates/diskclean-rules/     rule schema/engine and built-in rule packs
crates/diskclean-cleanup/   review basket, protected paths, revalidation, trash, audit
crates/diskclean-cli/       clap CLI and JSON output
apps/diskclean-gui/         eframe/egui application
rules/                      common/linux/windows TOML rule packs
docs/plan.md                durable UI/UX and feature delivery plan
docs/ui.md                  design system, layout, accessibility, snapshot plan
```

`diskclean-cleanup` owns all filesystem mutation: the `TrashBackend` trait,
protected-path policy, item snapshots/revalidation, and the JSONL audit log.
The GUI and CLI share the same audited apply pipeline. `diskclean-rules` stays
pure and never mutates the filesystem.

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
cargo run -p diskclean-cli -- recommend
cargo run -p diskclean-cli -- clean --dry-run
cargo run -p diskclean-cli -- duplicates . --json --min-size 1M
cargo run -p diskclean-gui -- .
cargo run -p diskclean-gui -- /path/to/directory
cargo check --workspace --target x86_64-pc-windows-gnu
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
- Keep dependencies permissive and GPL-compatible. Approved and in use:
  `egui`/`eframe`/`egui_extras`/`egui_kittest` (MIT/Apache-2.0), `rfd` (MIT),
  `trash` (MIT), Phosphor icons (MIT/Apache-2.0), Inter and JetBrains Mono
  (OFL-1.1), `sys-locale` (MIT/Apache-2.0), `blake3`
  (CC0-1.0/Apache-2.0), `toml`, `serde`, `serde_json`, `thiserror`, `clap`,
  `anyhow`, `ctrlc`, `rayon`, `tempfile`, `assert_cmd`, and `predicates`
  (MIT/Apache-2.0), `sysinfo` (MIT), `libc` (MIT/Apache-2.0; platform-only
  no-follow/nonblocking file-open flags).
  Approved for future phases when the feature actually lands:
  `czkawka_core` (MIT), `winreg` (MIT/Apache-2.0), `image`
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

Phase 1 GUI analyzer complete:

- [x] Volume picker, recent roots, path entry, and native folder picker
- [x] Background scan with progress and cancellation
- [x] Treemap with drill-down and breadcrumbs
- [x] Top files, top folders, extensions, and scan issues
- [x] Name, type, and minimum-size filters
- [x] In-memory cache of the three most recent scans
- [x] Responsive panels for narrow tiled windows
- [x] Linux smoke test; Windows target compiles and passes clippy
- [x] Linux release binaries built locally
- [x] Automated headless GUI tests
- [ ] Interactive Windows runtime test

UI/UX redesign U0–U2 complete (details in `docs/plan.md`):

- [x] Custom graphite/teal design system, dark/light/system themes, embedded
      Inter, JetBrains Mono, and Phosphor icons
- [x] Top command bar, navigation, central workspaces, contextual inspector,
      status bar, consistent empty/progress/error/toast states
- [x] Runtime English/Russian switch, persisted settings, panel widths, and
      recent roots
- [x] Analyze workspace with treemap, breadcrumbs, sortable virtualized tables,
      filters/chips, and keyboard shortcuts
- [x] `egui_kittest` shell tests for empty/analyze/cleanup/duplicates, commands
      filtering, keeper selection, stale-root invalidation, workspace
      shortcuts, and runtime theme/language switching
- [x] Automated palette contrast checks and 125%/200% high-DPI layout tests
- [x] `docs/ui.md` design system, interaction, accessibility, and snapshot plan
- [ ] U3 manual visual smoke, 100k performance pass, snapshots, screenshots

Safe cleanup implemented; safety stabilization in progress (2026-10-28):

- [x] Built-in common/Linux/Windows TOML rules, including recommendation-only
      system commands
- [x] Cleanup discovery, risk filters, safe-select, grouped cards, and a
      review basket with explicit confirmation
- [x] Protected-path policy, item snapshots, revalidation, OS-trash-only
      mutation, and JSONL intent/result audit
- [x] High-risk acknowledgement; system commands are never selectable or
      executed and are copy-only
- [x] CLI `recommend`, `clean --dry-run`, and audited `clean --apply --confirm`
- [x] Cleanup tests cover protected paths, changed files, symlink
      substitution, audit failures, partial failures, and fake-backend cancel

Exact duplicates implemented; scan-to-cleanup validation in progress:

- [x] Core size → bounded prefix hash → full BLAKE3 pipeline with progress,
      cancellation, bounded candidate collection, and issue reporting
- [x] Symlinks excluded; hardlink aliases skipped and counted; empty files
      ignored; files changed during hashing rejected
- [x] Duplicates workspace with minimum size, group cards, keeper selection,
      per-group/total savings, select-rest/all-except-keepers, review basket,
      and the shared audited trash pipeline
- [x] CLI `duplicates` with JSON, progress, cancellation, min size, hidden-file
      and filesystem options
- [x] Duplicate tests cover exact groups, same-size different content, prefix
      collisions, empty files, min size, hardlinks, symlinks, hidden files,
      unreadable directories, changed files, cancellation, and keeper savings

Verification on 2026-10-27: fmt, clippy with `-D warnings`, all workspace tests,
and the Windows target check pass. Test totals: cleanup 26, CLI 11, core 14,
scan integration 9, GUI 21, rules 4.

Later phases are not started: applications/games, similar photos/videos,
system recommendation polish, and packaging.

The 2026-10-28 review reopened safety and analyzer acceptance criteria. Follow
the S1–S5 stabilization checklist in `docs/plan.md` before adding later phases.
Known gaps include linked ancestors, directory descendants, timestamp precision,
duplicate keeper validation, cancellation, and analysis indexing/filtering.

S1 complete: linked ancestors/reparse points, precise timestamps, complete
directory manifests, mount boundaries, Windows roots, and post-audit validation
are covered. Cleanup/platform tests and Windows cross-check pass. See
`docs/safety.md` for the residual race inherent in path-based OS trash APIs.

## Roadmap

0. Foundation: scanner, tree model, CLI, tests, CI. **Complete.**
1. GUI analyzer: volumes, treemap, top files/folders, filters, cache.
   **Complete.**
2. UI/UX redesign: design system, shell, navigation, i18n, persisted settings,
   sortable virtualized tables, responsive inspector. U0–U2 complete; U3
   visual/snapshot/screenshot work remains.
3. Safe cleanup: temp/caches/trash/old downloads, review basket, audit log.
   **Complete.**
4. Exact duplicates: size -> prefix hash -> full BLAKE3, hardlink handling.
   **Complete.**
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
