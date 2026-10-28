# Disk Cleanup — UI/UX and feature delivery plan

Status: **active**. This file is the durable working plan. Update the status
table and the relevant milestone notes whenever work lands. `AGENTS.md` links
here so a fresh agent can recover context after a reset.

Last updated: 2026-10-28.

## Stabilization after the 2026-10-28 review

The original milestones below describe delivered functionality, not a completed
safety audit. Stabilization takes priority over applications and similar media.

- [x] S1: reject linked ancestors/reparse points and mount crossings; capture
      precise timestamps and complete directory manifests; recheck after audit
      intent; add adversarial regression tests and document residual OS races.
- [x] S2: retain duplicate scan evidence and verify both copies and keepers
      before trashing; validate opened-file identity on Windows.
- [x] S3: shared cancellable cleanup jobs in GUI/CLI, with per-item progress.
- [x] S4: full-tree analysis index, correct sorting, consistent filters and
      localized issue summaries; automated 100k-node performance coverage.
- [x] S5: correct platform user directories, documentation and locked CI builds,
      final Linux tests and Windows cross-check.

Real-display visual checks and interactive Windows testing remain separate
release gates and must not be marked complete based on cross-compilation.
The cargo-deny license gate remains part of packaging (Phase 7); cargo-deny is
not installed locally and no license audit is claimed by these stabilization checks.

## Status snapshot

| Area | Status | Notes |
|---|---|---|
| U0 design foundation | Complete | tokens, themes, fonts/icons, i18n, settings persistence, headless harness |
| U1 shell and navigation | Complete | command bar, navigation, inspector, status bar, recents/cache, progress/error states |
| U2 Analyze workspace | Complete | tabs, sortable virtualized tables, treemap, filters, inspector, keyboard shortcuts |
| U3 polish and quality | In progress | automated contrast, high-DPI, interaction, 100k-node headless rendering and Windows target checks pass; manual visual/performance smoke, snapshots and screenshots remain |
| Phase 2 safe cleanup | Complete | rules, discovery, protected paths, revalidation, trash-only apply, audit log, GUI and CLI |
| Phase 3 exact duplicates | Complete | BLAKE3 engine, hardlink/symlink handling, GUI keeper workflow, CLI `duplicates` |
| Parallel read operations | Complete | bounded metadata batches, concurrent duplicate hashing, persisted worker count and CLI flags; serial/parallel equivalence and cancellation coverage |
| Phase 4+ | Not started | applications/games, similar media, packaging |

## Product decisions locked for this plan

- UI languages: English and Russian, switchable at runtime; system locale is the
  default when it can be detected. All user-facing strings, rule explanations,
  plural forms, and error summaries are localized.
- Visual direction: custom design system, not stock egui. Dark, light, and
  system themes. Graphite surfaces, teal accent, Inter for text, a monospaced
  face for numeric columns, Phosphor icons.
- Work order: UI foundation first, then safe cleanup, then exact duplicates.
  Apps/games, similar photos, similar videos, system recommendations, and
  packaging follow.
- Cleanup is allowed to move user-selected files to the OS trash only after an
  explicit review-basket confirmation. System/admin cleanup remains
  recommendation-only and displays commands; it never executes them.
- No network access, telemetry, elevation, or permanent-delete fallback.

## Baseline UI problems (fixed by U0–U2)

- `apps/diskclean-gui/src/app.rs` is ~900 lines and owns the top bar, left
  navigation, filters, right inspector, and every analysis view.
- The left panel combines volumes, recent roots, cached scans, scan options,
  filters, and statistics. Recent roots and cached scans duplicate each other.
- All controls are always visible; there is no task-oriented grouping or
  progressive disclosure.
- Stock dark egui styling, random treemap colors, no consistent design tokens.
- Files/folders are rendered as `selectable_label` strings, without columns,
  sorting, or virtualized rows.
- Empty, loading, and failure states are plain text.
- No persisted settings, theme, language, panel widths, or window state.
- No entry points for cleanup, duplicates, or applications.
- `diskclean-rules` is a placeholder; no cleanup execution, audit log, or
  duplicate engine exists yet.
- No GUI tests.

## Target application shell

Three primary zones plus a global status bar:

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ ◆ Disk Cleanup   [/home/ilya/Downloads        ▾]  [Scan]          ⚙  ?       │
├──────────────────┬───────────────────────────────────────────┬───────────────┤
│ OVERVIEW         │ /home › ilya › Downloads      12.4 GiB    │ DOWNLOADS     │
│  ● /      120 GB │ [Map] [Files] [Folders] [Types] [Issues]  │ 12.4 GiB      │
│  ○ /home   80 GB │ [⌕ Search…] [Filters •2]                   │ 34% of /home  │
│                  │                                           │               │
│ RECENT           │  ┌─────────────────────────────────────┐  │ Logical       │
│  Downloads  2m   │  │                                     │  │ 12.9 GiB      │
│  Projects   1h   │  │             TREEMAP                 │  │ On disk       │
│                  │  │                                     │  │ 12.4 GiB      │
│ TOOLS            │  └─────────────────────────────────────┘  │ Files 1 284   │
│  Analyze      ●  │                                           │ Modified 2d   │
│  Cleanup         │                                           │               │
│  Duplicates      │                                           │ Largest       │
│  Applications    │                                           │ ████ 4.2 GiB  │
│                  │                                           │ ██   2.1 GiB  │
│ ⚙ Settings       │                                           │ [Open]        │
└──────────────────┴───────────────────────────────────────────┴───────────────┘
│ 12.4 GiB · 152,340 files · 3 issues · scan 4.2 s                    v0.2.0 │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Shell rules

- Top command bar: app mark, editable target path with recent-path dropdown,
  Browse, primary Scan/Cancel. Settings and help live here. No statistics or
  filters in this bar.
- Left navigation: sections **Overview** (volumes + one unified recent/cached
  list) and **Tools** (Analyze, Cleanup, Duplicates, later Applications).
  Settings/version/help at the bottom. Collapsible to an icon rail.
- Center: the selected workspace. Analyze has tabs; Cleanup has category
  sections; Duplicates has groups. One primary action per screen.
- Right inspector: contextual details, not a duplicate list. Node details,
  recommendation reason/risk, or duplicate-group keeper/savings.
- Status bar: global scan summary only; selection details stay in inspector.
- Keep previous scan results on screen while a rescan runs. Show a thin progress
  strip with cancel; swap results only on success. On failure keep the old tree
  and show a recoverable banner.
- Unified recents: recent roots and in-memory cached scans are one list with an
  age, size, and cached badge. Cached entry opens instantly; non-cached entry
  starts a scan.
- Scan options move to a settings dialog. Filters move to the workspace toolbar
  (search field + Filters popover + active filter chips).
- Empty state is an overview: volume cards with usage bars, recent roots, and
  quick selection. Never a blank treemap.
- Narrow layout: inspector becomes an overlay, sidebar collapses to a rail,
  nothing clips at 900x600 or at 125–150% scaling.

## Visual system

### Tokens

| Token | Dark | Light |
|---|---|---|
| `bg` | `#0E1116` | `#F5F7FA` |
| `panel` | `#131820` | `#FFFFFF` |
| `surface` | `#1A2029` | `#EEF2F6` |
| `surface_hover` | `#222B36` | `#E4EAF1` |
| `border` | `#2A3441` | `#D8E0E9` |
| `text` | `#E8EDF4` | `#15202B` |
| `text_muted` | `#8C99AA` | `#5E6C7D` |
| `accent` | `#5EEAD4` | `#0F766E` |
| `accent_soft` | `rgba(94,234,212,.14)` | `rgba(15,118,110,.12)` |
| `success` | `#5BD68A` | `#1F8A4C` |
| `warning` | `#F2B84B` | `#A86A0A` |
| `danger` | `#F06A6A` | `#C0392B` |
| `info` | `#6EA8FE` | `#2B6CB0` |

- Dark is the default; light and system modes are selectable and persisted.
- Accent is teal. Risk is always text + icon + color, never color alone.
- Treemap colors identify top-level branches, not node IDs. Nested tiles inherit
  a branch hue with lightness by depth. Palette must remain distinguishable for
  color-vision deficiencies.
- Typography: Inter at 20/16/13/11 px; numeric columns use a monospaced face.
  Spacing grid 4/8/12/16/24/32. Radii 8 for cards, 6 for controls. Table row
  height 30, header 32. Panel padding 12–16. Use surfaces and spacing instead of
  heavy dividers; 1 px borders are subtle.
- Native window decorations; no custom title bar in the first iteration.
- Icons: Phosphor regular at 16/18 px. Icon-only actions have tooltips.
- Dependency/license notes: `egui_extras` MIT/Apache-2.0; Phosphor font
  MIT/Apache-2.0 (embed the font directly if the `egui-phosphor` crate cannot
  match egui 0.37); Inter and JetBrains Mono OFL-1.1; `egui_kittest`
  MIT/Apache-2.0; `sys-locale` MIT/Apache-2.0. Add all to AGENTS.md and
  third-party notices.

## Component inventory

- `SectionHeader`: collapsible group heading.
- `NavItem`: icon, label, badge, selected pill.
- `VolumeCard`: name, mount point, usage bar, free/total, removable marker.
- `Button`: primary, secondary, ghost, danger, icon-only.
- `SegmentedControl`: analysis tabs.
- `FilterPopover` and `FilterChip`: one filter semantics across views.
- `DataTable`: columns, sort, hover, selection, context menu, virtualization.
- `RiskBadge`: safe/review/risky/command-only, icon + text.
- `StatRow`: label/value with numeric alignment.
- `EmptyState`, `ErrorState`, `Skeleton`.
- `ProgressStrip`: scan/hash/cleanup with cancel.
- `ToastHost`: success/error, auto-dismiss, details action.
- `ReviewBasket`: bottom bar for selected cleanup items.
- `ConfirmDialog`: list, total, risks, trash-only explanation, Cancel focused.
- `CommandCard`: system recommendation, exact command, copy button, no execution.

## Analysis workspace UX

- **Map**: breadcrumb, zoom/drill-down on double click, selected outline, hover
  tooltip. Selecting a file shows the parent treemap with the file highlighted
  and the file details in the inspector.
- **Files / Folders**: tables with Name, Size, % of root, Modified, Type.
  Sortable headers, middle-elided paths with tooltips, context menu Open,
  Reveal in file manager, Copy path.
- **Types**: sortable extension table with share bar.
- **Issues**: severity, path, message, copy, type filter.
- **Inspector**: object card, large size and percent of parent, Size, Contents,
  Metadata, Actions, top five children.
- **Filters**: search, file/folder type, minimum size, reset. Apply consistently
  across tabs. Active filters appear as removable chips.
- **Treemap**: branch colors, hover/selection, tooltip, drill-down, parent
  highlight, top-N plus an aggregate remainder tile when needed.
- **Tables**: use `egui_extras::TableBuilder` with virtualized `body.rows`;
  avoid per-frame path formatting and filtering. Cache an `AnalysisIndex` per
  tree.

## Keyboard and accessibility

`Ctrl+O` choose folder, `Ctrl+R`/`F5` rescan, `Ctrl+F` search, `Esc` close/reset,
`Backspace` up, `Enter` open selected, arrows navigate, `Ctrl+,` settings,
`Ctrl+1/2/3` switch workspaces, `Space` toggle cleanup item. `Delete` never
deletes; it opens the review basket where applicable.

All controls keyboard reachable, visible focus ring, AA contrast, AccessKit
labels. Validate both themes, 900x600, and HiDPI. Tooltips for truncated text.

## States

- First launch: volume/recents overview, no automatic scan.
- Scanning: previous data visible, progress strip, elapsed/counts/current path,
  cancel.
- Success: toast with duration/issues; cache updated.
- Failure: inline banner with reason, Retry, Choose another folder; old data
  preserved.
- No filter results: explanation and Reset filters.
- No recommendations/duplicates: explain what to scan and which settings matter.

## Technical GUI refactor

```text
apps/diskclean-gui/src/
  main.rs                // eframe entry point and window options
  app.rs                 // app state, scan jobs, routing, shortcuts
  settings.rs            // theme, language, panels, recent paths
  i18n.rs                // typed EN/RU keys and plural helpers
  dialogs.rs             // settings, help, toasts
  tests.rs               // headless egui_kittest regression tests
  ui/
    theme.rs             // palettes, Visuals, Style, fonts
    tokens.rs            // spacing, radii, sizes
    components.rs
    icons.rs
    format.rs            // size, percent, age, path shortening
  panels/
    top_bar.rs
    navigation.rs
    inspector.rs
    status_bar.rs
  views/
    analyze.rs           // workspace router + Analyze UI
    overview.rs          // empty overview
    cleanup.rs           // cleanup workspace and confirm dialog
    duplicates.rs        // duplicate groups, keepers, apply
  treemap.rs             // pure layout, keep tests
```

- Decouple `DiskCleanupApp::new` from `CreationContext` so the GUI can be tested
  without a window; keep a separate themed constructor for eframe.
- `AppState`: settings, scan state, active workspace/tab, filters, selection,
  inspector state.
- General `Job`/`JobControl` pattern: channel + progress + cancellation for
  scan, hashing, and cleanup. UI thread never blocks.
- `AnalysisIndex`: top files/folders, extensions, formatted rows; built once per
  tree rather than every frame.
- Persist settings, recent paths, panel widths, and window state with eframe's
  `persistence` feature.
- Localization: typed keys, interpolation, Russian plural forms, localized rule
  titles/descriptions, mapped `ScanError` summaries with technical details in a
  disclosure.
- Dependencies now in use: GUI `egui_extras`, embedded Phosphor/Inter assets,
  `sys-locale`; core `blake3`; rules `toml`; cleanup `trash`, `serde_json`,
  `thiserror`; CLI `clap`, `anyhow`, `ctrlc`; tests `tempfile`, `assert_cmd`,
  `predicates`, `egui_kittest`. Future: apps `winreg` on Windows and optional
  `rusqlite` for Lutris; similar `czkawka_core` optional, `image` for
  thumbnails.

## Functional architecture

- `diskclean-rules`: TOML schema, engine, built-in packs, recommendation model,
  risk, explanations. No filesystem mutation.
- New `diskclean-cleanup`: review plan, protected-path policy, revalidation,
  audit log, `TrashBackend` trait. Depends on core + platform + rules; GUI/CLI
  depend on it. Fake backend for tests.
- `diskclean-core`: exact-duplicate engine (size → prefix hash → full BLAKE3).
- `diskclean-platform`: trash, file-manager reveal, reparse-point facts, user
  dirs, registry/VDF/package reads. Keep low-level only.
- If applications or similar-media code grows, split into dedicated crates
  instead of growing platform indefinitely.

## Milestones

### U0 — Design foundation and refactor

- [x] Design tokens, dark/light/system palettes, typography, icon assets,
      formatters.
- [x] Split `app.rs` into shell/state/panels/views modules.
- [x] Settings + eframe persistence.
- [x] Typed i18n skeleton (EN/RU).
- [x] `egui_kittest` harness and synthetic tree fixture.
- [x] Fix Windows allocated size if it affects UI accuracy.

Exit: existing features preserved; theme/language switch; settings and panel
sizes survive restart; fmt, clippy, tests pass.

### U1 — Shell and navigation

- [x] Top command bar, left navigation, inspector, status bar.
- [x] Unified recents/cache, volume cards, empty overview.
- [x] Scan options in the Settings dialog.
- [x] Scan progress, cancel, preserve previous results, error/toast states.

Exit: no old control dump; one place per action; 900x600 and 1280x820 usable;
all existing functionality reachable.

### U2 — Analyze workspace

- [x] Tabs, sortable virtualized tables, context menus.
- [x] New treemap behavior and palette.
- [x] Inspector and breadcrumbs.
- [x] Filter popover/chips.
- [x] Keyboard scenarios: workspace switching, rescan, search, open, arrows,
      breadcrumbs, and native focused-checkbox toggling.

Exit: smooth on 100k+ nodes; no dead-end selection; copy/reveal work. Automated
interaction tests pass; the dedicated 100k-node GUI performance pass is tracked
under U3.

### U3 — Polish and quality

- [x] Automated light/dark contrast checks and high-DPI headless layout tests.
      Both palettes and treemap labels meet 4.5:1; 125% and 200% scaling are
      exercised at 900x600.
- [ ] Manual visual light/dark/HiDPI pass on a real display.
- [ ] EN/RU and dark/light snapshot tests. Runtime language/theme switch tests
      exist; pixel snapshots are not wired yet.
- [ ] 100k-node GUI performance pass.
- [ ] Linux X11/Wayland runtime smoke and interactive Windows smoke. The
      Windows target cross-check passes locally.
- [ ] Screenshots in README and `docs/ui.md`.
- [x] Write `docs/ui.md` with the design system, layout, interaction,
      accessibility, and snapshot strategy.
- [x] Update AGENTS.md and phase status.

## Remaining feature milestones

### Phase 2 — Safe cleanup, trash, audit log (complete)

- TOML rules with id/category/risk/localized title+description/matcher/action.
- Built-in common/Linux/Windows packs; custom user rules disabled initially.
- Categories: temporary, app caches, package caches, trash, old downloads,
  large old files, dev artifacts, system commands.
- Risks: low/medium/high/system-only. System/admin items only show exact
  commands; never execute them.
- Protected-path policy covers `/`, `/boot`, `/etc`, `/usr`, `/var/lib`,
  `/bin`, `/sbin`, `/lib*`, `/proc`, `/sys`, `/dev`, `/run`, user profile root;
  Windows `C:\`, Windows, Program Files*, ProgramData, System32, WinSxS,
  WindowsApps, user profile. Normalize `..`, case, UNC, `\\?\`.
- Refuse symlinks/junctions/reparse points. Revalidate `dev/ino/size/mtime`;
  for directories also mtime, direct-child count, and protected descendants.
- JSONL audit: write `intent` before mutation and `result` after; abort if
  intent cannot be written. XDG state / LocalAppData, 0600 on Unix, rotation,
  open-log action.
- Trash only; no permanent-delete fallback. Confirmation: basket → dialog with
  list/total/risks/high-risk acknowledgement → Move to Trash; Cancel focused.
- Cleanup workspace with summary, category sections, risk filters, safe-select,
  command cards, basket, per-item result, log/trash actions.
- CLI `recommend`, `clean --dry-run`; real cleanup only with explicit
  `--apply --confirm` and audit. Reuse `diskclean-cleanup`.

Exit: no protected path passes; every mutation confirmed and audited; fake
backend covers success/error/cancel/changed-file; system commands never run.

### Phase 3 — Exact duplicates (complete)

- Core engine: size buckets → prefix hash → full BLAKE3, streaming, cancellation,
  progress. Regular files only; symlinks and hardlink aliases excluded and
  counted as skipped aliases.
- Detect changes during hashing; skip unreadable groups with issues.
- Duplicates workspace: summary, min-size filter, group cards, keeper selection,
  potential savings relative to the chosen keeper. Nothing preselected; explicit
  “all copies except keeper” and “select rest”. Trash through the shared
  review/audit pipeline.
- CLI `duplicates --json`, min size, progress.
- Tests: empty, same-size different content, prefix collisions, hardlinks,
  symlinks, permission denied, changed files, minimum size, hidden files,
  cancellation, and keeper-based savings.

### Phase 4 — Applications and games

- Steam (libraryfolders.vdf, appmanifest), Epic, GOG, Heroic, Lutris; Windows
  Uninstall registry; Linux dpkg/pacman/flatpak/snap.
- Model: name, launcher, size, last played, version, leftover signals.
- Applications workspace grouped by launcher, inspector, leftovers section.
- Uninstall/commands recommendation-only; no elevation.
- Steam shader cache/Proton prefixes/leftovers are high-risk cleanup candidates
  with explanations. Fixture-based tests and Windows smoke.

### Phase 5 — Similar photos

- Dedicated feature-gated crate (`similar-photos`), `czkawka_core`, local
  thumbnails, perceptual hash groups, similarity threshold, keeper selection.
- Trash through the shared pipeline; size/count limits, cancellation, no network.

### Phase 6 — Similar videos

- Feature flag `similar-videos`; require system `ffmpeg`, explain and disable if
  absent. Extract frames to temp cache, hash, group, preview.
- Background-only, limits and cancellation; optional build if binary size matters.

### Phase 7 — System recommendations and packaging

- System command section: package caches, old kernels, Windows Update, recycle
  bin, journal. Exact command + explanation + copy; never execute.
- App icon, desktop file, metainfo, Windows icon/version resource.
- CI artifacts: Linux tar.gz/AppImage, Windows zip + MSI; SHA256 and third-party
  notices. `cargo-deny` licenses; no telemetry/network.
- Update README, docs/safety.md, docs/rules.md, AGENTS.md, CHANGELOG.

## Release order

| Version | Contents | Depends on |
|---|---|---|
| 0.2 | UI shell + analyze redesign + i18n/theme | — |
| 0.3 | Safe cleanup, trash, audit log | 0.2, rules + cleanup crate |
| 0.4 | Exact duplicates | 0.3, shared trash pipeline |
| 0.5 | Applications/games | 0.2, platform |
| 0.6 | Similar photos | 0.3, similar crate |
| 0.7 | Similar videos | 0.6, ffmpeg |
| 0.8 | System recommendations + packaging | all |

## Test and quality gates

- Unit: protected paths (Linux/Windows forms, `..`, case, UNC), revalidation,
  audit, rules, i18n plurals, filters.
- Fake trash backend: success/error/cancel/changed file/audit failure.
- Duplicates: hardlinks, symlinks, permission denied, changed during hash,
  cancellation.
- CLI: `recommend --json`, `clean --dry-run`, refusal without confirmation,
  exit codes.
- GUI: `egui_kittest` for empty/analyze/cleanup/duplicates/settings, EN/RU,
  dark/light; a few stable snapshots with tolerance.
- Manual: Linux X11 + Wayland, Windows 10/11, 100–150% scaling, 900x600, long
  paths, Cyrillic paths.
- Performance: 100k+ nodes smooth scroll/hover; bounded-memory hashing;
  cancellation; UI never blocks.
- Safety: no mutation without audit intent; no permanent delete; commands never
  executed.
- CI: fmt, clippy `-D warnings`, tests, release build, license check, Windows job.

## Defaults (adjust only with a documented decision)

- Teal accent, dark default, light/system available.
- Cleanup and Duplicates are separate workspaces.
- Settings is a modal; filters are in workspace toolbar; scan options in
  settings.
- Recent roots and cached scans are one list.
- No automatic scan when launched without a path argument.
- Dev artifacts (`target`, `node_modules`) are high-risk and unselected.
- New `diskclean-cleanup` crate; `diskclean-rules` never mutates.
- System paths are recommendation-only, never trash.
- No network, telemetry, auto-update, or elevation.

## Progress log

- 2026-10-28 (Windows CI fixture correction): Inspected failed run
  [40698710372](https://github.com/ValekusVachpekus/diskclean/actions/runs/40698710372)
  with `gh run view --log-failed`. Windows CLI recommendation/dry-run tests
  assumed overriding HOME redirected Known Folders and therefore never found
  their dev-artifact fixtures. Candidate assertions now use the common
  thumbnail-cache rule and its explicitly overridden XDG_CACHE_HOME, with an
  exact candidate-path/rule check. Child-process app-data/temp variables are
  isolated as well; production Known Folders resolution is unchanged. Core
  discovery tests still cover recursive dev-artifact discovery. Linux workspace
  tests and Linux/Windows-target all-targets clippy pass. Hosted Windows runtime
  verification of this correction remains pending until the change is pushed.

- 2026-10-28 (parallel scan and duplicate discovery): Added per-operation Rayon
  pools; automatic concurrency uses available CPUs capped at 8, manual settings
  accept 1–32 (1 is sequential). Scanner retains parallel directory traversal
  and now reads metadata in ordered batches of up to 256 entries. Duplicate
  discovery uses parallel metadata batches with serial hardlink/limit accounting,
  then schedules prefix and full-file hashing across all eligible size groups.
  Binary hash keys avoid per-file hex-string allocation. Each hashing worker
  uses one open file and a 64 KiB buffer; no nested worker pools are created.
  GUI EN/RU settings persist and apply to the next scan/search without restarting;
  CLI scan/largest/duplicates share `--threads` (0 = automatic). Cleanup mutation
  continues through the existing audited pipeline. Added serial/parallel full
  report equivalence, candidate-limit/hardlink/progress checks, active hashing
  cancellation, CLI count validation, and GUI setting interaction/persistence.
  Extended the release synthetic benchmark to 4,160 files, prefix-only and full
  hashing, five interleaved measured runs after warmup, including pool startup.
  Local warm-cache medians: 1 worker = 23 ms scan / 177 ms duplicates; automatic
  8 workers = 7 ms / 51 ms. These compare the new implementation's worker counts,
  not the previous release, and are not cold-cache/HDD results.
  Verification: fmt, workspace clippy with -D warnings, all 123 workspace tests
  (36 GUI), Windows cross-check and locked Linux workspace release build pass.
  Real-display and interactive Windows acceptance gates remain open.

- 2026-10-28 (U3 shell and layout consistency): Unified Cleanup/Duplicates
  headings and shared modal sizing/path-size rows. Navigation puts tools first
  and switches to a 60 px icon rail below 1000 logical pixels, preserving the
  full sidebar width across resize cycles. Inspector opens as an overlay when
  the dock would leave less than 560 px for the workspace. Command controls use
  a separate row below 760 px and reserve measured button widths. Long inspector
  names, child rows, status summaries and toasts are bounded; numeric columns
  remain distinct from paths. Analysis tables keep all columns available through
  horizontal scrolling; counts are numeric and filter controls wrap. Settings,
  help and trash confirmations fit the viewport with scrolling, including
  high-risk acknowledgements. Danger-button foregrounds now meet 4.5:1 contrast
  in both themes. Five new headless regressions cover command geometry, narrow
  analysis tabs, compact navigation/overlay/resize persistence and complete
  modal bounds at 600x480 and long recommendation text/basket geometry in EN/RU.
  Cleanup cards separate risk/size from wrapping titles; command copy controls
  reserve their own column. Manual real-display acceptance and pixel
  snapshots remain open. Verification: fmt, workspace clippy with -D warnings,
  all 117 workspace tests (35 GUI), Windows workspace cross-check and locked
  Linux release GUI build pass.

- 2026-10-28 (U3 duplicates polish): Groups now start collapsed and toggle from
  the header. Viewport culling covers both group cards and file rows inside
  expanded groups, with fixed geometry and stable widget IDs. Duplicate numeric
  summaries are cached until the report or keeper changes. Cleanup and Duplicates
  share vertical statistics cards with equal minimum heights and a responsive
  three/one-column grid, fixing inherited horizontal layout and text overflow.
  Current hashing paths occupy a separate truncated line to keep progress stable.
  Fixed “select all except keepers” using the displayed report (temporarily taken
  from workspace state while rendering); bulk selection no longer clones whole
  reports/file lists or searches for the keeper once per copy. Added tests for
  collapse/expand, bulk selection, EN/RU geometry in dark/light themes at
  480/760/1100 logical pixels, and scrolling an expanded 100,000-file group with
  fewer than 40 file rows rendered. Real-display visual/performance acceptance
  remains open. Verification: all 112 workspace tests (30 GUI), fmt, workspace
  clippy with -D warnings and Windows cross-check pass; Linux release GUI rebuilt.

- 2026-10-28 (U3 layout fix): Overview volume cards now explicitly lay out
  their contents vertically, end grid rows and switch to one column below
  660 logical pixels. Long mount/recent paths truncate with full tooltips;
  the heading and home action stack to fit narrow workspaces. Scan feedback
  fills left to right using scanned allocated file bytes divided by occupied
  space on the enclosing volume. The baseline is captured at scan start and
  the display is capped at 99% while processing/indexing. Percent and scanned /
  total bytes are shown; EN/RU tooltips explain the estimate for subfolders,
  skipped files and filesystem overhead. No pre-scan or branch budgeting. Added an EN/RU geometry regression with three volumes, long Docker
  mounts and 480/760/1100-pixel workspaces. Root `install.sh` builds a locked
  GUI release and symlinks `disk-cleanup` into `~/.local/bin` (or a supplied
  bin directory), refusing regular-file replacement. Real-display visual
  acceptance remains open.
  Verification: all 27 GUI tests (including byte progress and volume selection), fmt and
  GUI clippy with -D warnings pass. Release build and installation from a
  different working directory pass; regular-file replacement is rejected.

- 2026-10-28 (S5): User profile/Downloads and protected system locations use
  Windows Known Folders, including redirected folders and non-C installations.
  Linux Downloads honors XDG user-dirs.dirs without executing shell syntax;
  temporary-directory fallback uses the OS temp directory. CI builds/tests
  use the lockfile. Verification: fmt, clippy with -D warnings, all 106 tests
  and Windows workspace cross-check pass. The 100k-node index/query/headless
  render regression completed in 1.50 seconds locally (debug build; not a
  real-display scrolling benchmark). Locked offline Linux workspace release
  build passes; binaries are in `target/release`.

- 2026-10-28 (S4): Added a background-built full-tree AnalysisIndex retained
  with cached scans, cached query results and sorted child lists. Tables no
  longer limit search to the top 2,000 entries. Fixed sort direction, added
  extension sorting and compute type totals from individually filtered files.
  Issues support path/category filtering, localized summaries, technical
  detail tooltips and Copy path. Added a 100,000-node index/query/headless-render
  regression and tests for full-tree search and issue filters. Scanner also
  rejects linked root ancestors and detects same-device Linux bind mounts.

- 2026-10-28 (S3): Added cooperative apply cancellation during directory
  validation/content hashing and between trash calls. Both GUI workspaces use
  one background apply job with progress and Cancel; Escape cancels active
  apply, and CLI handles Ctrl+C. Concurrent GUI apply jobs are refused. Work
  already completed is still audited; cancellation does not interrupt an OS
  trash operation in flight. Added pre-start/between-items cancellation tests.

- 2026-10-28 (S2): Duplicate selections retain original scan metadata without
  filesystem work on the UI thread. Apply verifies BLAKE3 content and metadata
  for both copy and keeper; a keeper cannot be selected in the same batch.
  Keeper switches update the selected copies' evidence. Opened-file identity
  checks now run on Windows too, and hardlinked copies do not overstate savings.
  Added regressions for stale selections, missing/changed keepers, restored
  timestamps and same-size replacements. All workspace tests pass.

- 2026-10-28 (S1): Cleanup now rejects linked ancestors, all Windows reparse
  tags, mount roots/descendants and incomplete directory evidence. Full
  descendant manifests, nanosecond timestamps and link counts are revalidated
  after durable audit intent. Added eight cleanup regressions, a mount parser
  test and Windows drive/UNC-root cases. Cleanup/platform tests and Windows
  workspace cross-check pass. Residual path-based OS races are documented in
  `docs/safety.md`.

- 2026-10-23: Plan created; U0 started. Baseline is phase 0 + phase 1 GUI
  analyzer. Next: design tokens/theme, module split, i18n, persistence.
- 2026-10-27: U0–U2 completed. The shell, Analyze/Cleanup/Duplicates
  workspaces, responsive inspector, persistence, EN/RU switch, and keyboard
  navigation are implemented. Headless `egui_kittest` coverage now exercises
  empty/analyze/cleanup/duplicates states, commands filtering, keeper
  selection, workspace shortcuts, stale-root invalidation, and runtime
  theme/language switching.
- 2026-10-27: Phase 2 completed. Built-in TOML rule packs, discovery,
  protected paths, item snapshots, revalidation, trash-only mutation, JSONL
  audit, GUI review basket/confirmation, and CLI `recommend`/`clean`
  (`--dry-run`, `--apply --confirm`, `--audit-log`) are in place. System-only
  rules never enter the basket and commands are copy-only.
- 2026-10-27: Phase 3 completed. The core uses size buckets, bounded prefix
  hashing, and full BLAKE3; symlinks and hardlink aliases are excluded, empty
  files are ignored, and changed/unreadable files become issues. The GUI adds
  keeper selection, select-rest/all-except-keepers, per-group savings, and the
  shared audited trash pipeline. CLI `duplicates` supports JSON, progress,
  cancellation, minimum size, hidden-file and filesystem options.
- 2026-10-27: U3 automation advanced. Added palette contrast tests (all text,
  status colors, accent text, and treemap labels at 4.5:1 or better), adjusted
  the light status/accent colors and tile palette to meet the threshold, and
  added 125%/200% high-DPI headless layout tests at the 900x600 minimum. Wrote
  `docs/ui.md` and refreshed README. Manual visual smoke, a 100k-node
  performance pass, pixel snapshots, and screenshots remain.
- Verification on 2026-10-27: `cargo fmt --all`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --workspace`, and `cargo check
  --workspace --target x86_64-pc-windows-gnu` pass. Test totals: cleanup 26,
  CLI 11, core 14, scan integration 9, GUI 21, rules 4.
