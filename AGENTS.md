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
cargo run -p diskclean-cli -- applications /path/to/Steam --json
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
  no-follow/nonblocking file-open flags), and `rusqlite` (MIT) for Linux Lutris
  only behind platform's `lutris` feature, with bundled SQLite (public domain).
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

Safe cleanup implemented; S1–S3 safety stabilization complete (2026-10-28):

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

Exact duplicates implemented; scan-to-cleanup validation complete:

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

Applications/games has started with Steam inventory (see the 2026-10-29 milestone
below). Similar photos/videos, system recommendation polish and packaging have
not started.

The 2026-10-28 review reopened safety and analyzer acceptance criteria. Follow
the S1–S5 stabilization checklist in `docs/plan.md` before adding later phases.
The original gaps in linked ancestors, directory descendants, timestamp precision,
duplicate keeper validation, cancellation, and analysis indexing/filtering are
addressed by S1–S5 below.

S1 complete: linked ancestors/reparse points, precise timestamps, complete
directory manifests, mount boundaries, Windows roots, and post-audit validation
are covered. Cleanup/platform tests and Windows cross-check pass. See
`docs/safety.md` for the residual race inherent in path-based OS trash APIs.

S2 complete: duplicate selection retains scan evidence; apply rehashes copies
and keepers, rejects missing/changed keepers and conflicting batches. Windows
hashing validates opened-handle identity. All workspace tests pass.

S3 complete: shared GUI apply jobs expose progress and cancellation; CLI apply
handles Ctrl+C. Validation/hashing is cancellable and completed trash calls
are audited before stopping. Concurrent GUI apply operations are prevented.

S4 implemented: full-tree background analysis indexing and cached queries,
correct sort direction, filtered extension totals/sorting, localized issue
summaries and path/category filters. A 100k-node headless regression covers
indexing, query reuse and rendering; real-display performance remains a U3 gate.

S5 complete: Windows Known Folders resolves redirected profile/Downloads and
actual system paths; Linux honors XDG Downloads. CI uses locked dependencies.
Verification on 2026-10-28: fmt, clippy -D warnings, 106 tests and Windows
cross-check pass; Linux release binaries built. Test totals: cleanup 40, CLI 11, core 16, scan integration 9,
GUI 24, platform 2, rules 4. The 100k-node debug index/query/render check takes
1.50 s locally. Interactive smoke, snapshots/screenshots, cargo-deny and packaging
remain open; see `docs/plan.md`.

U3 follow-up (2026-10-28): overview grid/card overflow fixed, responsive
one/two-column cards and compact long paths added; scan progress now shows a
left-to-right byte-based progress bar instead of a zero-valued bar. The
estimate uses scanned allocated file bytes / occupied space of the enclosing
volume, fixed at scan start, with an approximate percentage and byte totals.
Tooltips explain that subfolders/skipped entries affect the estimate. EN/RU geometry
regression covers long mounts at three workspace widths. Root `./install.sh`
builds the release GUI and installs a `disk-cleanup` symlink in `~/.local/bin`
(optional bin directory argument); the checkout must remain in place.
Verification: 27 GUI tests, fmt, GUI clippy with -D warnings and Linux
release build pass; installer and symlink checked. Interactive visual
acceptance remains open.

U3 duplicates polish (2026-10-28): duplicate groups start collapsed and toggle
from their header. Only visible groups/file rows are laid out, including inside
very large expanded groups; stable IDs preserve keeper/checkbox interactions.
Numeric duplicate summaries are cached and invalidated on report/keeper changes.
Shared Cleanup/Duplicates statistics cards now stack their text vertically, use
equal minimum heights and switch to one column in narrow workspaces. Hashing
paths have their own truncated line to avoid progress layout jumps. Fixed bulk
selection against the displayed report and removed repeated keeper lookup and
report/file-list cloning during select-rest. Added EN/RU dark/light geometry,
collapse/expand/bulk-selection and 100k-file scrolling regressions. Manual
real-display smoothness/visual acceptance remains open. Verification: all 112
workspace tests (30 GUI), fmt, workspace clippy with -D warnings and Windows
cross-check pass; Linux release GUI rebuilt.

U3 shell consistency (2026-10-28): shared workspace headings, viewport-sized
scrollable dialogs and path/size rows; tools-first navigation with an adaptive
60 px icon rail and preserved sidebar widths; overlay inspector when docking
would squeeze the workspace; responsive command bar, analysis table scrolling,
numeric count columns and bounded filters/inspector/status/toasts. Destructive
button text now meets 4.5:1 contrast in both themes. Added EN/RU regressions for
480–1280 px command bars, narrow analysis tables, overlay/resize behavior and
complete 600x480 modal bounds and long recommendation/card/basket text.
Verification: fmt, workspace clippy with -D warnings, all 117 tests (35 GUI),
Windows workspace cross-check and locked Linux release GUI build pass.
Manual visual acceptance and pixel snapshots remain open; see `docs/plan.md`
and `docs/ui.md`.

Parallel read operations complete (2026-10-28): scanning now reads metadata in
bounded parallel batches alongside parallel directory traversal; duplicate
collection reads metadata in batches and hashes files concurrently across all
eligible size groups. Per-operation Rayon pools use available CPUs capped at 8
by default, support 1–32 manual workers (1 = sequential), and bound hashing to
one open file and a 64 KiB buffer per worker. EN/RU Settings → Scanning → Worker
threads persists and applies to new jobs; CLI scan/largest/duplicates share
`--threads` (0 = automatic). Concurrent jobs have separate pools. Candidate
limits, deterministic hardlink accounting, cancellation and the audited cleanup
pipeline remain intact. Regression tests compare serial/parallel reports and
progress, candidate limits and active hashing cancellation; GUI tests cover
selection/persistence and CLI tests cover flags/ranges. The synthetic release
benchmark compares worker counts with warm-cache medians (4,160 files): 1 vs 8
workers = 23 vs 7 ms scan, 177 vs 51 ms duplicates. These are not comparisons
against the previous release or predictions for HDD/cold-cache performance.
Verification: fmt, workspace clippy -D warnings, all 123 tests (36 GUI), Windows
cross-check and locked Linux workspace release build pass. No new dependencies.

Windows CI follow-up (2026-10-28): `gh` logs from run 40698710372 showed two
CLI tests depending on HOME overrides that Windows Known Folders ignores.
Recommendation/dry-run fixtures now use the common thumbnail-cache rule under
an explicitly overridden cache directory and assert the exact candidate/rule.
App-data/temp variables are isolated for child processes. Production profile
resolution is unchanged; core tests retain recursive dev-artifact coverage.
Linux workspace tests and all-targets clippy for Linux/Windows pass; hosted
Windows runtime verification awaits a push/rerun.

Source installation follow-up (2026-10-28): README includes full copy/paste
bootstrap blocks for Linux (Ubuntu/Debian, Fedora, Arch) and Windows 10/11 x64,
plus launch/update instructions. `install.bat [directory]` runs `install.ps1`:
locked MSVC release build, per-user executable copy and Start menu shortcut.
Windows installs default to LocalAppData/Programs/DiskCleanup and run independently
of the source checkout; Linux `install.sh` retains its checkout-backed symlink.
CI now checks Windows installer/README PowerShell syntax, installation into a
path with spaces and shortcut destination. Linux build/install, file-overwrite
refusal and README Bash syntax are locally checked. Windows installer execution
and fresh-machine bootstrap verification remain pending; binary packaging is
still open.

Phase 4 is the current user-directed feature work (2026-10-29); continue the
remaining providers/P2 rather than restarting U3 checks.

Phase 4 first Steam milestone (2026-10-29): platform owns bounded read-only
KeyValues parsing and common Steam root locations; core owns inventory and
installation-folder measurement using the existing scanner. Legacy/current
library metadata, explicit roots, installed/missing-folder/unknown status,
build ID and optional last-played facts are supported. GUI Applications has
background search/cancel, a virtualized list, localized ownership/size-scope
details, Copy path and Analyze drill-down (Ctrl+4). CLI `applications` exposes
the same JSON report with worker/mount options. Shared directories are not
summed; separate saves/Proton/shader caches are outside installation sizes.
Links/reparse points, unsafe metadata paths and unrequested mount crossings
are rejected. Inventory has no filesystem mutation or cleanup selection.
Other launchers/packages, P1 categories and P2 explained leftovers remain open.
See `docs/applications.md` for scope and fixtures.
Verification: fmt, Linux workspace all-targets clippy with -D warnings, all 137
workspace tests (39 GUI), Windows all-targets cross-clippy and locked Linux
workspace release build pass. Interactive Windows provider smoke remains open.

Applications UX follow-up (2026-10-29): inventory now uses a responsive,
virtualized card grid with left-aligned titles, prominent sizes and statuses.
Games/apps are separate from a collapsed group of recognizable Steam runtime,
Proton and redistributable names; grouping is presentation-only. Cards sort by
measured size within groups, with cached name/path search. Details open in a
bounded modal without moving the grid; source-path controls are collapsed and
refresh stays visible. EN/RU dark/light grid and modal interaction tests cover
narrow/wide workspaces and 100k-record scrolling. Verification: all 40 GUI tests,
fmt, GUI all-targets clippy with -D warnings, Windows GUI cross-check and locked
Linux release GUI build pass.

Epic inventory milestone (2026-10-29): user accepted the responsive card-grid
layout. Added offline `.item` metadata in platform and Epic inventory in core,
sharing the measured-size pipeline with Steam. Automatic native discovery probes
both Steam and Epic (Epic ProgramData on Windows); explicit roots select their
provider in GUI or CLI `--provider epic`. Provider labels/evidence are localized;
missing/incomplete/unverified installations stay distinct. Metadata reads are
bounded, links and mount crossings retain existing policy; no launch commands
are executed. Wine drive paths are not translated. `serde_json` (already an
approved workspace dependency) is now used by platform too. Next: Heroic/Legendary
inventory. Related folders/P2, remaining providers and Windows runtime smoke
remain open. Verification: fmt, all 144 workspace tests (41 GUI), Linux and
Windows all-targets workspace clippy -D warnings and locked Linux workspace
release build pass. See `docs/applications.md` and `docs/plan.md`.

Legendary/Heroic Epic milestone (2026-10-29): platform reads bounded local
`installed.json` (8 MiB, 20,000 records, serde_json recursion bound), validates
IDs/paths and retains valid rows alongside invalid-record issues. Core uses the
shared read-only size pipeline, cancellation and mount/link policy. Automatic
sources include native Legendary, Heroic and Linux Flatpak Heroic; explicit GUI
and CLI sources accept the config folder containing `installed.json`. A typed
`ApplicationSource` replaces the Steam/Epic boolean. Verification/preload flags
must be explicitly false to claim Installed; missing flags remain uncertain.
Source labels and explanations are localized. No new dependencies. Next: GOG
inventory including Heroic; Amazon, third-party records, P2 and Windows runtime
acceptance remain open. Verification: all 151 workspace tests (42 GUI), fmt,
Linux/Windows all-targets workspace clippy -D warnings and locked Linux workspace
release build pass. See `docs/plan.md`.

GOG/Heroic GOG milestone (2026-10-29): platform reads bounded Heroic installed
stores and `goggame-<id>.info` files. Core GOG provider lives in
`crates/diskclean-core/src/applications/gog.rs` and shares the existing read-only
size, cancellation and mount/link policy. Heroic GOG auto-detection uses native
and Flatpak roots; direct GOG takes an explicit game or shallow library folder.
Names use matching local info or folder names, shared installation sizes are not
summed, and gogdl resume markers keep status uncertain. GUI/CLI expose both
sources. Reads cap at 8 MiB/20k installed rows; shallow discovery caps at 100k
entries. No dependencies added. Galaxy registry and Linux `gameinfo`-only layouts
are not claimed; next source is Lutris. Verification: all 159 workspace tests
(43 GUI), fmt, Linux/Windows all-targets workspace clippy -D warnings and locked
Linux workspace release build pass. Interactive Windows smoke remains open.

Lutris milestone (2026-10-29): Linux `pga.db` installed-game inventory is wired
into GUI/CLI and native/Flatpak automatic discovery. `rusqlite` is now in use
only for Lutris, via platform's optional default `lutris` feature and Linux cfg;
SQLite is bundled, so users need no SQLite executable or development library.
SQLite reads a private bounded snapshot of database/WAL; it never opens the
source and cannot create its sidecars. Source identity/size/timestamps are
revalidated, rollback journals are rejected and snapshot/query/row limits plus
cancellation bound work. Owned temporary snapshots are removed on drop; this is
metadata-reader scratch storage, not user-data cleanup. Core provider code is in
`crates/diskclean-core/src/applications/lutris.rs`, with shared scanner size and
mount policy. Names, installed flags, directories and last played come from the
DB; recorded directories may contain Wine prefixes and shared files, explained
in EN/RU details. Windows Lutris inventory is unsupported and not auto-probed.
Next: Windows registry. Verification: all 167 workspace tests (44 GUI), fmt,
Linux/Windows all-targets workspace clippy -D warnings, platform no-default-feature
check and locked Linux workspace release build pass. See `docs/plan.md` and
`docs/dependencies.md`.

Windows Uninstall milestone (2026-10-29): native HKCU/HKLM 32/64-bit views
now supply read-only application names, versions and optional local installation
folders to GUI/CLI and automatic Windows discovery. Platform uses the existing
windows-sys dependency with Registry enabled; unsafe blocks are confined to its
FFI wrapper, with query/enumerate-only handles. Values, entry count and retained
text are bounded; hidden components and duplicate shared-view entries are
filtered. No uninstall commands or estimated sizes are read. Unknown folders
remain visible without Copy path/Analyze; registry_key is separate evidence,
not a filesystem path. Empty install_path/manifest_path represent absent paths
in these records and must never be passed to filesystem actions. Standard GOG
Uninstall records are included; Galaxy-specific keys remain open. Inventory
never proposes deletion. All 174 Linux workspace tests (45 GUI), fmt, Linux/Windows all-targets clippy
-D warnings and locked Linux workspace release build pass; native Windows
fixture execution/manual smoke remain pending. Next:
Linux packages, then P2. See docs/plan.md for verification and remaining scope.

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
5. Apps/games: **In progress.** Steam, Epic, Legendary and Heroic Epic inventory
   GUI/CLI implemented, plus Heroic GOG and explicit GOG `.info` folders. Next:
   Linux packages and leftovers (Lutris and Windows Uninstall implemented).
   GOG Galaxy registry, Linux `gameinfo`-only layouts and Heroic
   Amazon/third-party remain open.
6. Similar photos via `czkawka_core`.
7. Similar videos via `czkawka_core` + system `ffmpeg`.
8. System command recommendations and packaging polish.

User-approved additions (2026-10-28), all **planned, not implemented**; see
P1–P3 and acceptance criteria in `docs/plan.md`:

- P1: “Where did the space go?” category overview with readable names/icons,
  sizes and drill-down into games, apps, projects, downloads, media and caches;
  consistent totals and explicit unknown ownership.
- P2: application/game leftovers with ownership, installation status, content
  and removal/risk explanations. Extend Apps/games; protect saves and user data,
  and route reviewed cleanup through the existing audited trash pipeline.
- P3: exact-duplicate previews and explicit keeper proposals using preferred
  folders and newest/oldest rules; bulk review and per-group overrides, with
  no deletion selection merely from changing a rule. Similar-media phases stay
  separate.

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
