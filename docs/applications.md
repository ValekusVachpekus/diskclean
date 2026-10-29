# Applications and games

Phase 4 is in progress. Steam and Epic Games Launcher inventory are implemented. Discovery is read-only and
works offline; it never uninstalls games or creates cleanup selections.

## Usage

In **Applications** (`Ctrl+4`), leave the root empty to inspect common native
Linux/Flatpak and Windows Program Files Steam locations. Alternatively enter a
Steam installation or SteamLibrary root (the folder containing `steamapps`).
Click **Find applications** (or **Refresh list** after a search). Library path
controls are inside **Choose a source folder…**, with a Steam/Epic selector.
An empty root searches common native locations for both providers. Epic is
automatic on Windows; on Linux an explicit manifest directory is required. The previous inventory remains
visible while the background job runs; cancellation or a worker error preserves
it. Results use a responsive card grid with size and installation status.
Games/apps appear first, sorted by measured size. Recognizable local names for
Proton, Steam Linux Runtime and shared redistributables form a collapsed
**Steam components** group. This grouping is a display hint, not proof of
ownership or a cleanup recommendation. Name/path search includes both groups
and opens matching components automatically.

Click a card to open its details, copy its path or scan it in Analyze. Details
open in a scrollable modal so selecting a game does not move the grid; Close or
Escape dismisses it. Grid rendering only lays out visible rows, and grouped
search/sort results are cached between frames.

```bash
cargo run -p diskclean-cli -- applications /path/to/Steam --json --threads 4
cargo run -p diskclean-cli -- applications /path/to/Manifests --provider epic --json
```

CLI supports Ctrl+C and `--cross-filesystems`. GUI uses the persisted worker and
mount settings. Referenced libraries on another filesystem, including Linux bind
mounts, are skipped unless crossing is enabled. An explicit root can itself be
a mounted library. There is no automatic recursive search of arbitrary volumes.
Nonstandard Windows installations can be entered explicitly; registry discovery
is a later provider milestone.

## Evidence and accounting

- Names, app IDs, build IDs and optional last-played timestamps come from local
  `appmanifest_<id>.acf` files. These are application records and may include
  Steam runtimes/tools, not just games.
- Installed means the manifest has the fully-installed flag and the installation
  directory exists. A missing folder is reported as missing, not as proof that
  the game was uninstalled. Other flags or inaccessible folders remain unknown.
- Sizes come from scanning `steamapps/common/<installdir>`, not `SizeOnDisk` in
  the manifest. Allocated and logical bytes follow existing scanner hardlink
  accounting. Skipped links/mounts and read issues make measured sizes partial.
- Saves, `userdata`, Proton prefixes, shader caches and external content are
  separate. No whole-application or reclaimable-space claim is made.
- Two records may reference the same installation folder. Its measurement is
  reused, but the inventory deliberately provides no summed total that would
  double-count shared directories or files.
- Missing fields, unavailable libraries and malformed metadata become report
  issues; valid records remain available. Technical errors are available in
  GUI tooltips while primary summaries are localized.

## Epic metadata

Select **Epic Games** in source settings and enter the directory containing
`.item` files, normally `%ProgramData%/Epic/EpicGamesLauncher/Data/Manifests`
on Windows. Explicit CLI paths default to Steam; `--provider epic` requires an
explicit path. No-path discovery inspects both native providers. No Wine prefix
search or Windows-to-Unix drive mapping is performed; foreign paths produce
issues. Heroic's Legendary inventory is the next provider, not covered by this
Epic `.item` implementation.

`AppName`, `DisplayName`, `InstallLocation` and optional `AppVersionString` come
from local JSON. A record is installed only when the directory exists and both
`bIsIncompleteInstall` and `bNeedsValidation` are explicitly false. Missing flags,
incomplete installs and pending validation remain uncertain. Measurement uses
the same scanner/cache and partial-size semantics as Steam; `InstallSize` is
ignored. Launch commands, executables and authentication fields are ignored.
There is no aggregated total across records.

Reads use the no-follow regular-file wrapper and a 2 MiB cap. JSON nesting uses
serde_json's default recursion limit; duplicate known fields, wrong types,
relative paths, parent traversal and filesystem roots are rejected. Invalid
manifests become individual issues. Sources are deduplicated by directory
identity. Fixtures exercise actual sizes, shared folders, malformed data,
missing/incomplete installs, links, mount rejection, cancellation and CLI/GUI
source selection and localized provider evidence. The native Windows location
and Windows fixtures are cross-compiled; real Windows provider smoke is open.

Format reference: Legendary's own
[EGL manifest model](https://github.com/legendary-gl/legendary/blob/master/legendary/models/egl.py).
Only field names/format were consulted; no Python implementation is imported.

## Boundaries and tests

Platform owns local metadata access and the text KeyValues parser. Core maps
these facts into inventory records and scans content; GUI and CLI consume the
same report. Platform now uses the existing approved `serde_json` workspace dependency;
no new third-party package was added.

Both `steamapps/libraryfolders.vdf` and `config/libraryfolders.vdf` are inspected,
including legacy numbered strings and current numbered objects. Parsing is
case-insensitive for keys and handles comments, nested objects and quoted
backslash/quote escapes. Unsupported/malformed text is rejected, not guessed.
Each metadata read is capped at 2 MiB, nesting at 32 and entries at 100,000.
Manifest filenames must match app IDs. Installation directory names cannot
contain path separators, drive prefixes or parent traversal. Symlink/reparse
ancestors and files are rejected using platform wrappers. The residual rename
race of path-based reads is the same as the read-only scanner's.

Fixtures cover parser bounds, escapes, malformed input, path escapes and ID
mismatch; multiple/repeated libraries; installed/missing/uncertain records;
actual measured sizes, shared folders, hardlinks and linked paths; cancellation
and preservation of prior GUI reports. GUI tests cover EN/RU, both themes,
long text at narrow widths and virtualization of 100,000 records. Windows
cross-compilation does not replace interactive Windows testing.

Metadata format reference: the Steamlocate project's
[manifest source and example](https://docs.rs/steamlocate/latest/src/steamlocate/app.rs.html).
The parser here is implemented locally without importing another parser.

## Remaining Phase 4 work

Next: Heroic/Legendary installed-game metadata. GOG, Lutris, Windows registry
and Linux package discovery remain; related
folder ownership; P1 category totals; and P2 explained leftovers with explicit
review through the audited trash pipeline. No missing-manifest heuristic marks
saves or user data disposable. Uninstall actions remain recommendation-only.
