# Build and install the Windows GUI for the current user.
[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [string]$InstallDir = (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\DiskCleanup')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

try {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
        throw 'This installer requires Windows. On Linux, use install.sh.'
    }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw 'Cargo was not found. Follow the Windows installation block in README.md first.'
    }

    $InstallDir = [IO.Path]::GetFullPath($InstallDir)
    $targetDir = Join-Path $PSScriptRoot 'target'
    $binary = Join-Path $targetDir 'x86_64-pc-windows-msvc\release\diskclean-gui.exe'
    $destination = Join-Path $InstallDir 'disk-cleanup.exe'
    if (Test-Path -LiteralPath $destination -PathType Container) {
        throw "The executable destination is a directory: $destination"
    }

    & cargo build --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml') --locked --release `
        --package diskclean-gui --target x86_64-pc-windows-msvc --target-dir $targetDir
    if ($LASTEXITCODE -ne 0) {
        throw 'Build failed. Check the Rust version and the MSVC C++ tools/Windows SDK described in README.md.'
    }
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
        throw "The build did not produce the expected executable: $binary"
    }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item -LiteralPath $binary -Destination $destination -Force

    $programs = [Environment]::GetFolderPath('Programs')
    if ([string]::IsNullOrWhiteSpace($programs)) {
        throw 'Cannot locate the current user Start menu.'
    }
    New-Item -ItemType Directory -Force -Path $programs | Out-Null
    $shortcutPath = Join-Path $programs 'Disk Cleanup.lnk'
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($shortcutPath)
    $shortcut.TargetPath = $destination
    $shortcut.WorkingDirectory = $InstallDir
    $shortcut.Description = 'Disk Cleanup'
    $shortcut.Save()

    Write-Host "Installed: $destination"
    Write-Host 'Open Disk Cleanup from the Start menu.'
    Write-Host 'Close the running application before reinstalling or updating.'
    exit 0
} catch {
    Write-Error -Message $_.Exception.Message -ErrorAction Continue
    exit 1
}
