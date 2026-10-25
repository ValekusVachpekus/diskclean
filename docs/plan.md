# Disk Cleanup — UI/UX and feature delivery plan

Status: **active**. This file is the durable working plan. Update the status
table and the relevant milestone notes whenever work lands. `AGENTS.md` links
here so a fresh agent can recover context after a reset.

Last updated: 2026-10-23.

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

## Current UI problems (baseline)

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
  main.rs
  app.rs                 // update loop, routing, jobs
  state/
    settings.rs          // theme, language, panels, recent paths
    scan.rs              // ScanState, cache, derived index
    cleanup.rs
    duplicates.rs
  i18n/
    mod.rs
    en.rs
    ru.rs
  ui/
    theme.rs             // palettes, Visuals, Style
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
    analyze.rs
    treemap_view.rs
    cleanup.rs
    duplicates.rs
    apps.rs
  dialogs/
    settings.rs
    confirm.rs
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
- New dependencies: GUI `egui_extras`, embedded Phosphor/Inter assets,
  `sys-locale`; core `blake3`; rules `toml`, `globset`; cleanup `trash`,
  `serde_json`, `thiserror`; apps `winreg` on Windows and optional `rusqlite`
  for Lutris; similar `czkawka_core` optional, `image` for thumbnails.

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

- [ ] Design tokens, dark/light/system palettes, typography, icon assets,
      formatters.
- [ ] Split `app.rs` into shell/state/panels/views modules.
- [ ] Settings + eframe persistence.
- [ ] Typed i18n skeleton (EN/RU).
- [ ] `egui_kittest` harness and synthetic tree fixture.
- [ ] Fix Windows allocated size if it affects UI accuracy.

Exit: existing features preserved; theme/language switch; settings and panel
sizes survive restart; fmt, clippy, tests pass.

### U1 — Shell and navigation

- [ ] Top command bar, left navigation, inspector, status bar.
- [ ] Unified recents/cache, volume cards, empty overview.
- [ ] Scan options dialog.
- [ ] Scan progress, cancel, preserve previous results, error/toast states.

Exit: no old control dump; one place per action; 900x600 and 1280x820 usable;
all existing functionality reachable.

### U2 — Analyze workspace

- [ ] Tabs, sortable virtualized tables, context menus.
- [ ] New treemap behavior and palette.
- [ ] Inspector and breadcrumbs.
- [ ] Filter popover/chips.
- [ ] Keyboard scenarios.

Exit: smooth on 100k+ nodes; no dead-end selection; copy/reveal work.

### U3 — Polish and quality

- [ ] Light theme and contrast/HiDPI checks.
- [ ] EN/RU and dark/light snapshot tests.
- [ ] Linux X11/Wayland and Windows smoke.
- [ ] Screenshots in README, `docs/ui.md`.
- [ ] Update AGENTS.md and phase status.

## Remaining feature milestones

### Phase 2 — Safe cleanup, trash, audit log

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

### Phase 3 — Exact duplicates

- Core engine: size buckets → prefix hash → full BLAKE3, streaming, cancellation,
  progress. Regular files only; symlinks and hardlink entries excluded; hardlink
  groups badged as zero savings.
- Detect changes during hashing; skip unreadable groups with issues.
- Duplicates workspace: summary, min-size filter, group table, keeper selection,
  potential savings. Nothing preselected; explicit “all copies except keeper”.
  Trash through the shared review/audit pipeline.
- CLI `duplicates --json`, min size, progress.
- Tests: empty, same-size different content, hardlinks, symlinks, permission
  denied, changed during hash, cancellation.

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

- 2026-10-23: Plan created; U0 started. Baseline is phase 0 + phase 1 GUI
  analyzer. Next: design tokens/theme, module split, i18n, persistence.
