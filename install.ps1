<#
.SYNOPSIS
    Cyrene installer for Windows — the AI agent that always loves you.

.DESCRIPTION
    Downloads the prebuilt cyrene.exe for your architecture from GitHub Releases
    and installs it to a user-local bin directory (added to PATH). Works on
    Windows PowerShell 5.1+ and PowerShell 7+, x64 and ARM64.

    Quick install:
        irm https://raw.githubusercontent.com/YourWisemaker/cyrene/master/install.ps1 | iex

.PARAMETER Version
    Specific version to install (e.g. "0.1.0"). Defaults to the latest release.

.PARAMETER InstallDir
    Where to place cyrene.exe. Defaults to "$env:LOCALAPPDATA\Cyrene\bin".
#>
[CmdletBinding()]
param(
    [string]$Version = $env:CYRENE_VERSION,
    [string]$InstallDir = $env:CYRENE_INSTALL_DIR
)

$ErrorActionPreference = "Stop"
$Repo = "YourWisemaker/cyrene"

Write-Host "Cyrene Installer — the AI agent that always loves you" -ForegroundColor Cyan
Write-Host ""

# --- Detect architecture -> Rust target triple ---
$arch = $env:PROCESSOR_ARCHITECTURE
switch ($arch) {
    "AMD64" { $target = "x86_64-pc-windows-msvc" }
    "ARM64" { $target = "aarch64-pc-windows-msvc" }
    "x86"   { throw "32-bit x86 Windows is not supported. Use a 64-bit system." }
    default { throw "Unsupported architecture: $arch" }
}

# --- Resolve version ---
if ([string]::IsNullOrWhiteSpace($Version)) {
    Write-Host "Resolving latest version..."
    try {
        $rel = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" `
            -Headers @{ "User-Agent" = "cyrene-installer" }
        $Version = $rel.tag_name -replace '^v', ''
    } catch {
        throw "Could not determine the latest version. Set -Version explicitly. ($_)"
    }
}
$Version = $Version -replace '^v', ''
Write-Host "Installing cyrene v$Version ($target)..."

# --- Install location ---
if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    $InstallDir = Join-Path $env:LOCALAPPDATA "Cyrene\bin"
}
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

# --- Download + verify + extract ---
$asset = "cyrene-$target.zip"
$url = "https://github.com/$Repo/releases/download/v$Version/$asset"
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("cyrene-" + [System.Guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
$zipPath = Join-Path $tmp $asset

try {
    Write-Host "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $zipPath -UseBasicParsing

    # Best-effort checksum verification.
    try {
        $shaUrl = "$url.sha256"
        $shaFile = "$zipPath.sha256"
        Invoke-WebRequest -Uri $shaUrl -OutFile $shaFile -UseBasicParsing
        $expected = (Get-Content $shaFile -Raw).Split(" ")[0].Trim().ToLower()
        $actual = (Get-FileHash $zipPath -Algorithm SHA256).Hash.ToLower()
        if ($expected -and ($expected -ne $actual)) {
            throw "Checksum mismatch (expected $expected, got $actual)."
        }
        Write-Host "Checksum verified."
    } catch {
        Write-Host "  (checksum verification skipped: $_)" -ForegroundColor DarkGray
    }

    Expand-Archive -Path $zipPath -DestinationPath $tmp -Force
    $exe = Get-ChildItem -Path $tmp -Recurse -Filter "cyrene.exe" | Select-Object -First 1
    if (-not $exe) { throw "cyrene.exe not found in the downloaded archive." }
    Copy-Item $exe.FullName (Join-Path $InstallDir "cyrene.exe") -Force
    Write-Host "Installed to $InstallDir\cyrene.exe" -ForegroundColor Green
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

# --- Ensure InstallDir is on the user PATH ---
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$InstallDir", "User")
    $env:Path = "$env:Path;$InstallDir"
    Write-Host "Added $InstallDir to your user PATH (restart terminals to pick it up)."
}

Write-Host ""
Write-Host "Installation complete! Run 'cyrene --help' to get started." -ForegroundColor Green
Write-Host "Next: run 'cyrene onboard' to configure a model provider and channel."
