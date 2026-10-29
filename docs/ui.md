# Disk Cleanup UI

This document describes the current GUI design system, shell layout, interaction
rules, and the headless verification strategy. It is the reference for new
screens; stock egui styling is not the default.

Status: U0–U2 complete. Automated contrast, high-DPI, interaction, and target
checks pass. A manual visual smoke pass, pixel snapshots, and screenshots remain
open in [`plan.md`](plan.md).

## Design principles

- **One place per action.** Primary actions live in the top command bar or the
  current workspace; the inspector is contextual and never duplicates global
  commands.
- **Density with breathing room.** Tables and treemaps stay compact, while
  grouped cards and the review basket keep destructive actions visually
  separated.
- **Risk is visible before selection.** Cleanup cards show category and risk;
  high-risk items require explicit acknowledgement and system commands are
  copy-only.
- **Calm by default.** Graphite surfaces, a teal accent, and semantic colors
  only for status, risk, and destructive actions.
- **Offline and local.** No telemetry, no network access, no elevation, and no
  permanent-delete fallback.
- **Localized at runtime.** Every user-facing string goes through the typed
  EN/RU catalog. Rule explanations use the localized rule text.

## Palette

The palette is defined in `apps/diskclean-gui/src/ui/theme.rs`. Both themes
use the same semantic names.

| Token | Dark | Light |
|---|---|---|
| `bg` (workspace) | `#0E1116` | `#F5F7FA` |
| `panel` (bars, inspector) | `#131820` | `#FFFFFF` |
| `surface` (cards, controls) | `#1A2029` | `#EEF2F6` |
| `surface_hover` | `#222B36` | `#E4EAF1` |
| `border` | `#2A3441` | `#D8E0E9` |
| `text` | `#E8EDF4` | `#15202B` |
| `muted` | `#8C99AA` | `#556373` |
| `accent` | `#5EEAD4` | `#0B6B63` |
| `on_accent` | `#062A26` | `#FFFFFF` |
| `on_danger` | `#0E1116` | `#FFFFFF` |
| `success` | `#5BD68A` | `#14703C` |
| `warning` | `#F2B84B` | `#8B5500` |
| `danger` | `#F06A6A` | `#B73225` |
| `info` | `#6EA8FE` | `#2A66A6` |

Treemap tiles use a six-color sequence per theme. Tile labels are white. Both
the six tile colors and the semantic text colors are covered by automated
contrast tests.

Palette rules:

- Body, muted, status, and treemap text must reach at least **4.5:1** contrast
  against every surface it can appear on.
- Filled accent buttons use `on_accent`; filled danger buttons use `on_danger`.
  Both foreground/background pairs must reach 4.5:1 contrast.
- Semantic colors are reserved for status and risk. They are not decoration.
- Do not introduce new raw colors in screens; add a token and test its
  contrast.

## Typography and icons

| Role | Face | Size |
|---|---|---|
| Display / workspace title | Inter SemiBold | 20 px |
| Body | Inter Regular | 13 px |
| Button / emphasized label | Inter Medium | 13 px |
| Small label, metadata | Inter Regular | 11.5–12 px |
| Section label | Inter Medium, uppercase | 10.5 px |
| Numeric columns | JetBrains Mono Regular | 11–11.5 px |
| Phosphor icons | embedded Phosphor | 16 px (28 px hit area) |

Font files live in `apps/diskclean-gui/assets/fonts/`; the Phosphor icon font
lives in `apps/diskclean-gui/assets/icons/`. Icons are referenced through
`ui/icons.rs` constants. Do not use Unicode emoji as UI icons.

## Spacing and geometry

| Token | Value |
|---|---|
| `SPACE_2` | 8 |
| `SPACE_3` | 12 |
| Card radius | 8 |
| Control radius | 6 |
| Table row height | 30 |
| Table header height | 32 |
| Default sidebar width | 236 (resizable 196–320) |
| Default inspector width | 300 (resizable 248–440) |
| Status bar height | 26 |

The minimum supported window is **900x600**. The Analyze inspector is shown
automatically on wide windows only when at least 560 logical pixels remain for
the workspace. Narrow windows open the inspector as a modal overlay. Below
1000 logical pixels, navigation becomes a 60 px icon rail; returning to the full
sidebar restores its previous width. Workspace links come before volumes and
recent roots so many mounted volumes cannot bury the tools. Below 760 px the
command bar uses a second row for the path and scan controls. The shell is also
exercised at 125% and 200% pixels-per-point in headless tests.

## Shell layout

```text
┌──────────────────────────────────────────────────────────────┐
│ command bar: path, scan/rescan, search, settings, help        │
├──────────────┬───────────────────────────────┬───────────────┤
│ navigation   │ workspace                     │ inspector     │
│ Analyze      │ treemap / tables / cards      │ selection     │
│ Cleanup      │                               │ metadata      │
│ Duplicates   │                               │ actions       │
├──────────────┴───────────────────────────────┴───────────────┤
│ status bar: scan state, counts, size, issues, root            │
└──────────────────────────────────────────────────────────────┘
```

### Workspaces

- **Analyze** (`Ctrl+1`): overview, treemap, files, folders, types, issues.
- **Cleanup** (`Ctrl+2`): recommendation discovery, risk filters, grouped
  cards, safe-select, review basket.
- **Duplicates** (`Ctrl+3`): minimum-size search, progress/cancel, group cards,
  keeper selection, review basket.
- **Applications** (`Ctrl+4`): Steam root/autodetection, background inventory,
  responsive virtualized card grid, separate collapsed Steam components, cached
  search by name/path and size ordering. Cards open ownership/installation
  details in a bounded modal with Analyze drill-down.

Navigation shows only implemented workspaces. Do not add disabled placeholders
for future phases unless they explain what is coming and why they are disabled.

## Components

- **Primary button:** accent fill, `on_accent` text, 30 px minimum height.
- **Secondary button:** surface fill, border stroke, 30 px minimum height.
- **Icon button:** 28x28 px hit area, tooltip or accessible label required.
- **Navigation item:** 32 px height, accent-soft selection, 3 px accent bar,
  icon plus label, optional count badge.
- **Chip:** rounded 10 px, surface fill, removable filters sit in the filter
  row.
- **Card:** panel/surface fill, 1 px border, 8 px radius, 8–12 px inner
  spacing.
- **Workspace heading:** shared icon/title/subtitle component with wrapping
  subtitles and consistent typography.
- **Table:** 30 px virtualized rows, sortable headers, monospaced numeric
  columns, right-aligned sizes. A horizontal viewport preserves all columns
  below the 560 px table budget. Count columns contain numbers; their headers
  identify the unit. Long names truncate and expose full paths on hover.
- **Review basket:** persistent selection summary with total size, clear
  selection, confirmation, and apply result. It never auto-selects items.
- **Confirm dialog:** Cancel is focused when the dialog opens; destructive
  confirmation is disabled while busy; high-risk cleanup needs a separate
  acknowledgement. Shared viewport sizing and vertical scrolling keep settings,
  shortcuts, confirmations and the inspector within the window. Confirmation
  rows reserve the size column before truncating paths.
- **Toast:** short success/info/error message with an icon; detailed errors
  remain in the workspace error state.
- **Empty/progress/error states:** every workspace must explain the current
  state and the next available action.

## Keyboard and accessibility

| Shortcut | Action |
|---|---|
| `Ctrl+1` | Analyze |
| `Ctrl+2` | Cleanup |
| `Ctrl+3` | Duplicates |
| `Ctrl+4` | Applications |
| `Ctrl+R`, `F5` | Rescan the current root |
| `Ctrl+F` | Focus search |
| `Enter` | Open the selected item |
| arrows | Move through visible siblings |
| `Backspace` | Go to the parent |
| `Esc` | Close dialog, clear selection, or cancel the active search |

Focused native checkboxes can be toggled with `Space`. Icon-only controls set
an accessible label. Truncated paths and labels need a tooltip. Color is never
the only carrier of risk: cards also use text labels, and destructive controls
are explicitly named.

## Safety UX

- Scanning is read-only.
- Nothing is selected automatically. “Select safe” selects low/medium-risk
  trash items only.
- System/admin cards are recommendation-only; their commands can be copied but
  never enter the review basket.
- High-risk items need a separate acknowledgement before confirmation.
- The confirmation dialog lists the action, item count, total size, and
  destination (OS trash). The default is never permanent deletion.
- Apply progress and partial failures are reported per item. The audit log path
  is shown in the result.
- If the audit log is unavailable, cleanup apply is disabled before mutation.

## Localization

Analyze searches all indexed paths, including files outside the scan's top-N
summary. Types applies search/minimum-size/file visibility to individual files
before aggregating extensions; share bars use the filtered total. Issues uses
path search and its own category selector; data size/type filters do not hide
unreadable objects. Localized summaries expose original OS details on hover.

- Language is switchable at runtime and persisted; system locale is the
  default when detectable.
- All labels, tooltips, empty states, dialogs, errors, and rule descriptions
  are localized. Use plural helpers for counts instead of concatenating
  fragments.
- Keep numeric columns and file paths unchanged; only format sizes, dates, and
  labels.
- Test both languages at the minimum window size. Long Russian labels must not
  clip primary actions.

## Headless verification

`apps/diskclean-gui/src/tests.rs` drives the real shell through `egui_kittest`
without opening a desktop window. Current coverage includes:

- empty overview, Analyze tabs and selection, minimum window size;
- runtime theme and language switching, palette contrast, and 125%/200% high-DPI
  layout at 900x600 in English and Russian;
- cleanup rendering, no preselection, safe-select, high-risk acknowledgement,
  command-only cards, and filter empty states;
- duplicate rendering, keeper/select-rest behavior, stale-root invalidation;
- workspace shortcuts and fixture consistency;
- command actions at 480/760/900/1280 px with long paths in EN/RU;
- narrow analysis tables, active filters and long names/issue paths;
- icon-rail navigation, overlay inspector and sidebar widths across resizing;
- complete modal bounds at 600x480, including high-risk confirmations;
- long recommendation titles/descriptions and review-basket geometry in EN/RU
  and both themes.

Run the GUI tests with:

```bash
cargo test -p diskclean-gui
```

### Snapshot and screenshot plan

Pixel snapshots are not wired yet. Because snapshots need a GPU renderer, keep
them optional and deterministic:

1. Render a fixed set of states at 1280x820 and 900x600, light and dark, EN and
   RU: empty overview, Analyze map, Analyze folders, cleanup with basket,
   duplicate groups, and confirm dialog.
2. Use the embedded fonts, disable animations and cursor blinking, and pin the
   OS shortcut convention.
3. Store only the approved baseline images; review diffs before updating.
4. Capture README screenshots from the same states so documentation and tests
   cannot drift apart.

Until snapshots are enabled, use the headless interaction tests plus a manual
visual pass on Linux X11/Wayland and Windows. Manual checks must cover:

- light/dark/system themes and 125%/150%/200% scaling;
- 900x600 and 1280x820 windows;
- English and Russian, including long paths and rule explanations;
- keyboard-only navigation, focus visibility, and dialog Escape handling;
- treemap labels, muted metadata, disabled controls, and risk colors;
- no clipped primary actions or accidental horizontal overflow; table
  horizontal scrolling remains available when columns need more space.

## File map

```text
apps/diskclean-gui/src/
  app.rs                 app state, scan jobs, routing, shortcuts
  settings.rs            persisted theme/language/layout/recent roots
  i18n.rs                typed EN/RU strings and plurals
  dialogs.rs             settings, help, toasts
  tests.rs               headless egui_kittest regression tests
  ui/theme.rs            palettes, typography, egui style
  ui/tokens.rs           spacing, radii, type scale, layout sizes
  ui/components.rs       reusable shell primitives
  ui/icons.rs            Phosphor codepoints
  ui/format.rs           size/date/path formatting
  panels/                top bar, navigation, inspector, status bar
  views/                 overview, analyze, cleanup, duplicates
  treemap.rs             pure treemap layout
```

### Worker preferences (2026-10-28)

Settings → Scanning → Worker threads is localized in EN/RU and persists with
other preferences. Automatic displays its effective worker count (available CPU
parallelism capped at 8); manual choices are 1–32, with 1 meaning sequential.
The setting applies to new scan/duplicate jobs without restarting. Existing
jobs keep their initial count. Explanatory text notes that HDDs may benefit
from fewer workers. The selector has an accessible label; headless interaction
tests cover switching manual/automatic in both languages, and modal bounds
remain covered at 600×480.

Applications provider follow-up (2026-10-29): accepted card layout retained.
Source settings now choose Steam or Epic for explicit folders; an empty source
searches both native providers. Card status lines include the launcher, details
show its own metadata explanation, and Epic never enters the Steam components
group. EN/RU dark/light tests cover source selection and Epic details.
