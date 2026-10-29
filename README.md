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
- [x] Steam/Epic/Legendary and Heroic Epic inventory with measured sizes (GUI + CLI)
- [ ] Other launchers, packages and explained application leftovers
- [ ] Similar photos/videos
- [ ] Packaging

## Features

### Applications and games

Open **Applications** (`Ctrl+4`) to search common Steam, Epic, Legendary and
Heroic Epic locations, or select a source folder in the collapsed settings.
Steam accepts a Steam/SteamLibrary folder; Epic accepts its `Manifests` folder
with `.item` files (automatic detection on Windows). Legendary and Heroic Epic
accept the config folder containing `installed.json`; native and Linux Flatpak
Heroic locations are detected. Heroic GOG/Amazon are separate, pending providers.
Search measures installation sizes in the
background, supports cancellation and opens individual games in Analyze.
Results appear as responsive cards, with games first and Steam components
collapsed. Search covers names and paths; a card opens details without moving
the grid. Installation status and metadata evidence are explained in English/Russian.
Separate saves, Proton data and shader caches are outside these sizes; shared
files mean per-app sizes must not be added together. Other launchers and
leftover cleanup are planned. [Scope and safety](docs/applications.md).

```bash
cargo run -p diskclean-cli -- applications /path/to/Steam --json
cargo run -p diskclean-cli -- applications /path/to/Manifests --provider epic --json
cargo run -p diskclean-cli -- applications /path/to/legendary --provider legendary --json
cargo run -p diskclean-cli -- applications /path/to/heroic/legendaryConfig/legendary --provider heroic --json
```


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
- Keyboard shortcuts: `Ctrl+1/2/3/4` workspaces, `Ctrl+R`/`F5` rescan, `Ctrl+F`
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

## Installation

The commands below build the GUI from source and install it for the current
user. The first installation needs internet access, several GB of free space,
and time to compile dependencies. The application itself works offline.
System dependencies may ask for your sudo password or show a Windows UAC prompt.
Rust **1.95 or newer** is required; these commands install the stable toolchain.

### Linux (Ubuntu / Debian, Fedora, Arch Linux)

Open a terminal, copy **this entire block**, and paste it once. It installs
build dependencies for your distribution, Rust, and the application:

```bash
bash <<'INSTALL_DISK_CLEANUP'
set -euo pipefail
if command -v apt-get >/dev/null 2>&1; then
    sudo apt-get update
    sudo apt-get install -y build-essential git curl pkg-config \
        libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-dev \
        libegl1-mesa-dev libgl1-mesa-dev
elif command -v dnf >/dev/null 2>&1; then
    sudo dnf install -y gcc gcc-c++ make git curl pkgconf-pkg-config \
        libxkbcommon-devel libxkbcommon-x11-devel wayland-devel libX11-devel \
        mesa-libEGL-devel mesa-libGL-devel
elif command -v pacman >/dev/null 2>&1; then
    sudo pacman -Syu --needed --noconfirm base-devel git curl pkgconf \
        libxkbcommon libxkbcommon-x11 wayland libx11 mesa libglvnd
else
    echo 'Install the Rust and eframe build dependencies for your distribution first.' >&2
    exit 1
fi
if ! command -v rustup >/dev/null 2>&1 && [ ! -x "$HOME/.cargo/bin/rustup" ]; then
    rustup_script="$(mktemp)"
    trap 'rm -f -- "$rustup_script"' EXIT
    curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "$rustup_script"
    sh "$rustup_script" -y --profile minimal --default-toolchain stable
fi
export PATH="$HOME/.cargo/bin:$PATH"
rustup toolchain install stable --profile minimal
source_dir="$HOME/.local/share/disk-cleanup-source"
mkdir -p "$(dirname "$source_dir")"
git clone https://github.com/ValekusVachpekus/diskclean.git "$source_dir"
RUSTUP_TOOLCHAIN=stable bash "$source_dir/install.sh"
echo "Ready. Launch with: $HOME/.local/bin/disk-cleanup"
INSTALL_DISK_CLEANUP
```

Run the installed application:

```bash
~/.local/bin/disk-cleanup
~/.local/bin/disk-cleanup ~/Downloads
```

The Linux installer creates a `disk-cleanup` symlink in `~/.local/bin` pointing
to the release GUI inside `~/.local/share/disk-cleanup-source`. **Keep that source
folder in place.** If `~/.local/bin` is in PATH, you can simply run `disk-cleanup`.
An optional argument to `install.sh` selects another bin directory; existing
regular files are never overwritten.

If you already cloned the project and installed its dependencies:

```bash
./install.sh
```

To update an installation made with the full block above:

```bash
bash <<'UPDATE_DISK_CLEANUP'
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
rustup toolchain install stable --profile minimal
git -C "$HOME/.local/share/disk-cleanup-source" pull --ff-only
RUSTUP_TOOLCHAIN=stable bash "$HOME/.local/share/disk-cleanup-source/install.sh"
UPDATE_DISK_CLEANUP
```

### Windows 10 / 11 (64-bit)

Open **PowerShell** as your normal user, copy **this entire block**, and paste
it once. `winget` must be available (install/update **App Installer** from the
Microsoft Store if the command is missing). Accept the UAC prompt for the
Microsoft C++ Build Tools. The block installs Git, Rust and the C++ workload,
clones the project, and calls `install.bat`:

```powershell
& {
    $ErrorActionPreference = 'Stop'
    if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
        throw 'Install App Installer from the Microsoft Store, then paste this block again.'
    }
    function Install-Dependency([string]$Id, [string[]]$Extra = @()) {
        & winget install --id $Id --exact --source winget --accept-package-agreements --accept-source-agreements @Extra
        # WinGet also returns this code when the installed version is already current.
        if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335189) {
            throw "Dependency installation failed: $Id (exit $LASTEXITCODE)."
        }
    }
    Install-Dependency 'Git.Git'
    Install-Dependency 'Microsoft.VisualStudio.2022.BuildTools' @('--override', '--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended')
    Install-Dependency 'Rustlang.Rustup'
    $env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + [Environment]::GetEnvironmentVariable('Path', 'User') + ';' + $env:Path
    $env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-msvc'
    & rustup toolchain install stable-x86_64-pc-windows-msvc --profile minimal
    if ($LASTEXITCODE -ne 0) { throw 'Rust installation failed.' }
    $sourceDir = Join-Path $env:LOCALAPPDATA 'DiskCleanup-source'
    & git clone https://github.com/ValekusVachpekus/diskclean.git $sourceDir
    if ($LASTEXITCODE -ne 0) { throw 'Clone failed. If the source folder already exists, use the update block below.' }
    & (Join-Path $sourceDir 'install.bat')
    if ($LASTEXITCODE -ne 0) { throw 'Disk Cleanup installation failed.' }
    Start-Process (Join-Path $env:LOCALAPPDATA 'Programs\DiskCleanup\disk-cleanup.exe')
}
```

The installer copies the GUI to
`%LOCALAPPDATA%\Programs\DiskCleanup\disk-cleanup.exe` and creates a **Disk Cleanup**
shortcut in your **Start menu**. It installs for the current user without adding
to PATH. The installed Windows application runs independently of the source
folder. `install.bat` delegates to [install.ps1](install.ps1); its execution-policy
bypass applies only to that PowerShell process and does not change system policy.

If C++ Build Tools were already installed without the C++ workload, open
**Visual Studio Installer → Modify → Desktop development with C++**, include a
Windows SDK, and rerun `install.bat`. See the official
[Rust Windows prerequisites](https://rust-lang.github.io/rustup/installation/windows-msvc.html)
and [Visual Studio installation parameters](https://learn.microsoft.com/en-us/visualstudio/install/use-command-line-parameters-to-install-visual-studio).
If an installer requests a restart, restart Windows before rerunning the build.

If you already cloned the project and have Rust/MSVC installed, open PowerShell
in the repository and run:

```powershell
.\install.bat
# Optional custom installation directory:
.\install.bat "D:\Apps\Disk Cleanup"
```

To update an installation made with the full block above, close Disk Cleanup
and paste this into PowerShell (keep the source folder if you want to update):

```powershell
& {
    $sourceDir = Join-Path $env:LOCALAPPDATA 'DiskCleanup-source'
    & git -C $sourceDir pull --ff-only
    if ($LASTEXITCODE -ne 0) { throw 'Source update failed.' }
    $env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-msvc'
    & rustup toolchain install stable-x86_64-pc-windows-msvc --profile minimal
    if ($LASTEXITCODE -ne 0) { throw 'Rust update failed.' }
    & (Join-Path $sourceDir 'install.bat')
    if ($LASTEXITCODE -ne 0) { throw 'Disk Cleanup update failed.' }
}
```

## Build and run (development)

```bash
cargo build --release --locked
cargo run -p diskclean-cli -- volumes
cargo run -p diskclean-cli -- scan ~ --json
cargo run -p diskclean-gui -- .
cargo run -p diskclean-gui -- ~/Downloads
```

The GUI accepts an optional path; when provided it starts scanning immediately.
Native release binaries are written to `target/release/diskclean` and
`target/release/diskclean-gui` (with `.exe` on Windows). The Windows installer
uses the explicit MSVC target under `target/x86_64-pc-windows-msvc/release`.

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
