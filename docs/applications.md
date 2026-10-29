# Applications and games

Phase 4 is in progress. Steam, Epic Games Launcher, Legendary and Heroic Epic
inventory are implemented, plus Heroic GOG and explicit GOG `.info` sources.
Discovery is read-only and works offline; it never uninstalls games or creates cleanup selections.

## Usage

In **Applications** (`Ctrl+4`), leave the root empty to search common launcher
locations. Alternatively select a provider and enter its source folder: Steam
or SteamLibrary (containing `steamapps`), Epic `Manifests`, or Legendary config
(containing `installed.json`, also used by Heroic Epic). GOG accepts game/library
folders with `.info` files; Heroic GOG accepts its Heroic config root.
Click **Find applications** (or **Refresh list** after a search). Library path
controls are inside **Choose a source folder…**, with a provider selector.
An empty root searches the automatic sources; standalone GOG folders remain
explicit. Epic is automatic on Windows;
on Linux its `.item` directory must be explicit. The previous inventory remains
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
cargo run -p diskclean-cli -- applications /path/to/legendary --provider legendary --json
cargo run -p diskclean-cli -- applications /path/to/heroic/legendaryConfig/legendary --provider heroic --json
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
explicit path. No-path discovery inspects all implemented sources. No Wine prefix
search or Windows-to-Unix drive mapping is performed; foreign paths produce
issues. Heroic Epic uses the separate Legendary format below.

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

## Legendary and Heroic Epic

Both providers read **only `installed.json`**, an object keyed by app ID. Choose
**Legendary** or **Heroic (Epic)** and enter the folder containing this file.
The **Heroic (Epic)** selector covers this backend; **Heroic (GOG)** is a
separate implemented source below. Amazon and third-party lists are not read. Account catalogs, `user.json`, executable/launch commands,
save paths and authentication are never consulted or executed.

Automatic locations:

- Standalone Legendary: absolute `LEGENDARY_CONFIG_PATH`, otherwise
  `$XDG_CONFIG_HOME/legendary`, otherwise `~/.config/legendary` (also on Windows,
  following Legendary's own configuration layout).
- Heroic: `$XDG_CONFIG_HOME/heroic/legendaryConfig/legendary` or
  `~/.config/heroic/legendaryConfig/legendary` on Linux;
  `%APPDATA%/heroic/legendaryConfig/legendary` on Windows.
- Linux Flatpak Heroic:
  `~/.var/app/com.heroicgameslauncher.hgl/config/heroic/legendaryConfig/legendary`.

Relative environment overrides are ignored. Metadata paths retain link and
mount checks. Reads are capped at 8 MiB and 20,000 records with the default JSON
recursion limit. Duplicate top-level IDs and malformed JSON reject the file;
individual invalid rows produce issues while valid neighbours survive. IDs must
match `app_name`, titles must be nonempty and installation paths must be native,
absolute, below a filesystem root and without parent traversal. Foreign Wine
paths are rejected rather than translated.

Names/versions are metadata evidence; sizes come from the existing scanner,
not `install_size`. Directory measurements are reused within a source operation.
Hardlinks, skipped entries and partial sizes use the shared scanner semantics.
Installed requires an existing directory with `needs_verification` and
`is_preloaded` explicitly false. Missing flags, verification or preload mean
uncertain; an absent folder means missing. Records from different sources stay
separate even when they name the same game; there is no summed space claim.
DLC records may share the base game's installation directory.

Fixtures cover mixed valid/invalid rows, ID mismatch, bad paths/types, bounds,
duplicate IDs, shared directories/hardlinks, status flags, missing folders,
links/mounts, cancellation, CLI source isolation and localized GUI source
selection/details. Windows runtime provider testing remains open.

Format references: Legendary's
[installed-game model](https://github.com/legendary-gl/legendary/blob/master/legendary/models/game.py),
[local configuration layout](https://github.com/legendary-gl/legendary/blob/master/legendary/lfs/lgndry.py),
and Heroic's
[Legendary paths](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/storeManagers/legendary/constants.ts).
The implementation reads local metadata directly; no launcher code is imported.

## GOG and Heroic GOG

**Heroic (GOG)** reads `<heroic-config>/gog_store/installed.json`, an object
with an `installed` array. Use `--provider heroic-gog` with the Heroic config
root, not its Epic/Legendary subfolder. Automatic detection uses native Heroic
roots and Linux Flatpak roots described above, and skips absent optional stores.
Only installed records are consumed; account libraries, auth and launch commands
are not used. Invalid rows or repeated IDs produce issues while valid rows
survive. `appName` must be a numeric GOG ID and `install_path` a native absolute
path without traversal or a filesystem root. Reads cap at 8 MiB, 20k records and
the default JSON recursion limit.

**GOG** (`--provider gog`) accepts an explicit game folder or a library whose
game folders are its immediate children. It looks for `goggame-<id>.info` in
those folders only. The numeric `gameId` must match the filename; names and
optional build IDs come from local JSON. Discovery is capped at 100k directory
entries, cancellable and subject to mount/link checks. It does not probe drives
recursively or query the Galaxy registry. Native Linux installations with only
`gameinfo` and no `.info` file are a remaining format extension.

For Heroic GOG, matching `.info` improves name/build evidence; otherwise the
installation folder supplies a readable name. A present installed record and
directory indicate Installed; malformed optional info or a `.gogdl-resume`
marker makes it uncertain. Direct GOG uses matching local info plus directory
presence as installation evidence. This is not content-integrity verification.
Missing Heroic installation folders stay Missing, not disposable leftovers.
Sizes measure the complete installation folder with the shared scanner; they
do not use `install_size`. Multiple base/DLC records can share a measured folder;
there is no summed inventory total. Separate saves/prefixes are not included.

```bash
cargo run -p diskclean-cli -- applications /path/to/GOG-Games --provider gog --json
cargo run -p diskclean-cli -- applications /path/to/heroic --provider heroic-gog --json
```

Fixtures cover malformed/bounded metadata, filename mismatches, invalid rows,
shared installation folders/hardlinks, fallback names, missing folders,
unfinished downloads, links/mounts, source isolation and EN/RU GUI source/details
in both themes. Native Windows runtime smoke remains open.

Format references: Heroic's
[GOG installed-store definition](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/storeManagers/gog/electronStores.ts),
[GOG info format](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/common/types/gog.ts),
and [local install handling](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/src/backend/storeManagers/gog/library.ts).
Only formats were consulted; launcher code is not imported.

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

Next: Lutris. GOG Galaxy registry, native Linux `gameinfo`-only layouts,
Heroic Amazon/third-party,
Windows registry and Linux package discovery remain; related
folder ownership; P1 category totals; and P2 explained leftovers with explicit
review through the audited trash pipeline. No missing-manifest heuristic marks
saves or user data disposable. Uninstall actions remain recommendation-only.
