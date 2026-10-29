# Applications and games

Phase 4 is in progress. Steam, Epic Games Launcher, Legendary and Heroic Epic
inventory are implemented, plus Heroic GOG, explicit GOG `.info` sources and
Linux Lutris, Windows Uninstall records and dpkg/pacman packages.
Discovery is read-only and works offline; it never uninstalls games or creates cleanup selections.

## Usage

In **Applications** (`Ctrl+4`), leave the root empty to search common launcher
locations. Alternatively select a provider and enter its source folder: Steam
or SteamLibrary (containing `steamapps`), Epic `Manifests`, or Legendary config
(containing `installed.json`, also used by Heroic Epic). GOG accepts game/library
folders with `.info` files; Heroic GOG accepts its Heroic config root.
Click **Find applications** (or **Refresh list** after a search). Library path
controls are inside **Application sources…**, with a provider selector.
An empty root searches the automatic sources (except selecting Windows registry
searches that provider only); standalone GOG folders remain
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
Nonstandard launcher directories can be entered explicitly. Registry-only
discovery takes no source folder.

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
there is no summed inventory total. External saves/prefixes are not included; anything inside the recorded folder
is part of its measurement.

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

## Lutris (Linux)

The **Lutris** source uses the data folder containing `pga.db`, not the YAML
config `games` folder. Automatic roots are `$XDG_DATA_HOME/lutris` (fallback
`~/.local/share/lutris`) and Flatpak's
`~/.var/app/net.lutris.Lutris/data/lutris`. Custom Lutris `pga_path` settings are
not parsed; an explicit source must contain a database named `pga.db`.

```bash
cargo run -p diskclean-cli -- applications /path/to/lutris-data --provider lutris --json
```

Only the `games` table's `installed=1` rows are inventoried; account-wide
`service_games`, YAML launch commands and credentials are ignored. IDs, names,
recorded installation directories and optional last-played timestamps are local
metadata evidence. Invalid rows are counted as issues. Installed means the
record is marked installed and its directory exists; there is no integrity
check or uninstall inference. Missing folders stay Missing. Sizes use the shared
read-only scanner/cache and are never summed across records. A recorded folder
may contain a Wine prefix, saves or shared content: all content inside it is
included, while external related data is excluded. No ownership or reclaimable
space is inferred from the whole directory.

The platform `lutris` feature is enabled by default, but only Linux compiles the
approved `rusqlite` dependency. SQLite is bundled; no external SQLite program or
system library is required. Windows automatic discovery omits Lutris; explicit
Lutris source requests produce an unsupported-source issue.

SQLite never opens the original database. The reader makes a private temporary
snapshot of `pga.db` and any `-wal` through no-follow regular-file handles;
source `-shm` is not opened. The snapshot can create its own sidecars only inside
its owned temporary directory, removed on drop. Source metadata is captured and
rechecked after copying: identity, size and precise modification timestamps.
A nonempty rollback journal, changed source, linked sidecar or malformed database
is rejected with an issue and can be retried after the writer finishes. This is
an optimistic read snapshot; the scanner's residual path/rename races still
apply, and no stable cross-process transaction is claimed for a malicious writer.

Each source DB/WAL file caps at 64 MiB; results cap at 20,000 installed rows and
1 MiB SQLite values, with a 10-million VM-operation budget. Copies check
cancellation between 64 KiB reads; SQLite checks it through a progress callback.
Only fixed SELECT/PRAGMA statements are used, trusted schema is disabled and
`games` must be a real table. The original database and its WAL/SHM bytes are
covered by non-mutation tests, including committed WAL-only records. Other
fixtures cover source changes, corruption, bounds, invalid/uninstalled rows,
missing/shared folders, hardlinks, links/mounts, cancellation and GUI/CLI source
selection in EN/RU and both themes.

Format references: Lutris's
[database schema](https://github.com/lutris/lutris/blob/master/lutris/database/schema.py)
and [data-path settings](https://github.com/lutris/lutris/blob/master/lutris/settings.py).
See [dependency notes](dependencies.md) for the added SQLite dependency.

## Windows Uninstall registry

Automatic discovery on Windows includes current-user (`HKCU`) and machine-wide
(`HKLM`) `Software\Microsoft\Windows\CurrentVersion\Uninstall`, explicitly
using both WOW64 registry views. Select **Windows registry** in Applications to
search only this source, without a folder. Other users' hives, offline registry
exports and Store/MSIX packages are not probed. On other platforms this explicit
source reports unsupported and is excluded from automatic discovery.

```powershell
cargo run -p diskclean-cli -- applications --provider registry --json
```

Names and versions come from `DisplayName` and `DisplayVersion`. Entries without
a name and hidden `SystemComponent` registrations are skipped. Identical records
from shared views are deduplicated using hive, product subkey and metadata;
conflicting records and distinct products remain separate. Standard Uninstall
registrations from GOG are included, without inferring a launcher from a name or
folder. Dedicated Galaxy vendor keys remain future work.

`InstallLocation` is optional. Only a local drive-absolute, non-root directory
without traversal, device/UNC prefixes or unresolved environment variables is
eligible for scanning; a matching outer quote pair is accepted. No guesses come
from executable icons or uninstall strings. Missing/unsupported locations retain
an unknown status/size and have no Copy path or Analyze action. A recorded usable
folder that exists means Installed; a missing folder means Missing, not proof
that uninstalling completed. The usual no-follow, hardlink, mount, partial-size
and cancellation rules apply to each recorded directory as a scan root.
Registry registrations do not prove integrity, exclusive ownership or removable
bytes; overlapping folders/providers can report the same files.

JSON adds `registry_key` (hive, view, subkey) as evidence. `manifest_path` is empty
for registry records; `install_path` is empty when no usable directory is known.
Empty paths are absent evidence, never a request to scan the current directory.
Details show the source key with a tooltip and explain management through Windows
Settings/the original installer. No uninstall controls or cleanup selections are
created.

Native handles request query/enumerate rights only. Each value read is bounded
to 32 KiB, all views together to 20,000 enumerated keys and 16 MiB of metadata
text. Reaching a limit returns the partial inventory with an issue. Individual
access/malformed-value errors preserve other entries. Enumeration is cancellable
between entries/measurements and is a live observation, not a transactional
registry snapshot. No registry data is modified; no `UninstallString`,
`DisplayIcon` or `EstimatedSize` values are read. The existing `windows-sys`
Registry API feature adds no dependency.

Linux-executable fixtures cover native-path validation, shared/hardlinked folder
sizes, missing/unknown folders, linked installs, evidence and cancellation.
CLI verifies incompatible path arguments and unsupported hosts. EN/RU dark/light
GUI tests cover the no-folder source and absent path actions. A Windows-only test
creates its own uniquely named HKCU fixture, verifies Unicode/invalid/oversized
values, hidden components and that reader handles cannot write, then removes the
fixture. That test has been cross-compiled locally; execution awaits Windows
CI/runtime. Interactive Windows inventory acceptance is still open.

Format/access references: Microsoft documents
[Uninstall values](https://learn.microsoft.com/en-us/windows/win32/msi/uninstall-registry-key)
and [alternate registry views](https://learn.microsoft.com/en-us/windows/win32/winprog64/accessing-an-alternate-registry-view).

## Linux packages: dpkg and pacman

Automatic Linux discovery reads `/var/lib/dpkg/status` and the local pacman
`/var/lib/pacman/local/*/desc` database when present. Explicit sources are the
database directories, never `/usr`, a package archive or the pacman sync DB.
Explicit metadata folders are readable on both supported platforms; automatic
system locations are Linux-only. Custom dpkg admindir / pacman DBPath must be
entered explicitly; package-manager configs and environment overrides are not
interpreted.

```bash
cargo run -p diskclean-cli -- applications /var/lib/dpkg --provider dpkg --json
cargo run -p diskclean-cli -- applications /var/lib/pacman/local --provider pacman --json
```

dpkg includes only records whose status reports `ok installed`, including held
packages; residual `config-files`, unpacked and reinstreq/broken states are
excluded. Pacman records come from the local installed database, with NAME,
VERSION and ARCH; known repository-only desc fields are rejected. IDs include
architecture, so dpkg multiarch records remain distinct. Duplicate package IDs
produce issues and retain the first record; they are not silently added twice.
Versions are metadata, not integrity verification. Valid neighbors survive
malformed records or missing desc files; a corrupt dpkg stanza is skipped.

Package files can span protected system directories and shared dependencies.
These sources **do not scan `/usr` or invent a single installation folder**.
`allocated_bytes`/`logical_bytes` stay null and `install_path` is empty.
`manifest_path` identifies the status/desc metadata. The optional JSON
`reported_size_bytes` stores the database estimate: dpkg `Installed-Size` in
1024-byte units, pacman `SIZE` (or legacy `ISIZE`) in bytes. Missing size stays
unknown; invalid/overflowing sizes produce an issue without discarding the
package. Conflicting SIZE/ISIZE rejects a desc record.

System packages have a dedicated initially collapsed group so dependencies do
not crowd the games grid. Search expands matching packages. Cards use ≈ and a
localized Package estimate label; details separately show unknown measured
usage, declared size, version, package/architecture identity and source evidence.
There is no Copy path/Analyze action for an absent installation folder. Estimates
are neither reclaimable bytes nor additive physical usage: filesystem allocation,
maintainer scripts, dependencies and separately created user data affect reality.
Removal remains the responsibility of the package manager; inventory creates
no cleanup candidate and executes no commands, scripts or network operations.

Reads use no-follow regular-file handles and check source/opened identity,
size and precise mtime around bounded cancellable reads. dpkg status is capped
at 64 MiB / 20,000 stanzas; desc files at 1 MiB; lines at 64 KiB and fields at
256. Core discovery also bounds total declared metadata input per provider call to 64 MiB,
per-directory entries and accepted package count to 20,000, rejects linked
ancestors/mount crossings and deduplicates root aliases. Excessive individual
sources produce issues. Per-file checks do not make a multi-file/live package
database transactional: a package-manager update can invalidate the observation;
refresh after it completes. Dependency/ownership graphs and content verification
are not inferred from these records.

Fixtures cover installed/held/removed/broken dpkg states, multiarch, continuation
fields, duplicates, malformed/overflow/missing sizes, both pacman size formats,
contiguous sections, links, cancellation, source changes and limits.
CLI tests prove estimates remain separate and source text is unchanged. GUI
coverage checks EN/RU dark/light collapsed groups, search, source controls and
no path/cleanup actions. A native pacman smoke read 1691 packages with zero
issues (1682 sizes reported, zero measured). Flatpak/Snap and RPM remain open.

Format references: Debian's
[status fields](https://www.debian.org/doc/debian-policy/ap-pkg-controlfields.html)
and [Installed-Size](https://www.debian.org/doc/debian-policy/ch-controlfields.html#installed-size),
and Arch's [local desc format](https://man.archlinux.org/man/alpm-db-desc.5.en).

## Boundaries and tests

Platform owns local metadata access and the text KeyValues parser. Core maps
these facts into inventory records and scans content; GUI and CLI consume the
same report. Platform uses the approved `serde_json` workspace dependency and, for Linux
Lutris only, feature-gated `rusqlite` with bundled SQLite.

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

Next: Flatpak/Snap. Galaxy-specific registry discovery, native Linux
`gameinfo`-only layouts and Heroic Amazon/third-party remain open; related
folder ownership; P1 category totals; and P2 explained leftovers with explicit
review through the audited trash pipeline. No missing-manifest heuristic marks
saves or user data disposable. Uninstall actions remain recommendation-only.
