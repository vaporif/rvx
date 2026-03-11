#!/usr/bin/env pwsh
$ErrorActionPreference = 'Stop'

$Repo = "vaporif/rvx"
$Binary = "rvx"
$InstallDir = if ($env:RVX_INSTALL_DIR) { $env:RVX_INSTALL_DIR } else { "$env:USERPROFILE\.local\bin" }

# GitHub API auth (optional, avoids rate limits)
$Headers = @{}
if ($env:GITHUB_TOKEN) {
    $Headers["Authorization"] = "Bearer $env:GITHUB_TOKEN"
}

# Detect architecture
$Arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
switch ($Arch) {
    "X64"   { $Target = "x86_64-pc-windows-msvc" }
    "Arm64" { $Target = "aarch64-pc-windows-msvc" }
    default { Write-Error "Unsupported architecture: $Arch"; exit 1 }
}

# Get latest release tag
$Latest = $null
try {
    $Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers $Headers
    $Latest = $Release.tag_name
} catch {}

if (-not $Latest) {
    try {
        $Releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases" -Headers $Headers
        $Latest = $Releases[0].tag_name
    } catch {
        Write-Error "Could not determine latest release. Set GITHUB_TOKEN if rate-limited."
        exit 1
    }
}

# Archive naming: rvx-<target>-<tag>.zip
$Archive = "$Binary-$Target-$Latest.zip"
$Url = "https://github.com/$Repo/releases/download/$Latest/$Archive"

Write-Host "Installing $Binary $Latest for $Target..."
Write-Host "Downloading $Url..."

# Download and extract
$TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) "rvx-install-$([System.Guid]::NewGuid())"
New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null

try {
    $ZipPath = Join-Path $TmpDir "archive.zip"
    Invoke-WebRequest -Uri $Url -OutFile $ZipPath -UseBasicParsing

    Expand-Archive -Path $ZipPath -DestinationPath $TmpDir -Force

    # Find binary
    $BinPath = Get-ChildItem -Path $TmpDir -Filter "$Binary.exe" -Recurse | Select-Object -First 1
    if (-not $BinPath) {
        Write-Error "Binary '$Binary.exe' not found in archive"
        exit 1
    }

    # Install
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item -Path $BinPath.FullName -Destination (Join-Path $InstallDir "$Binary.exe") -Force

    Write-Host "Installed $Binary to $InstallDir\$Binary.exe"

    # Add to user PATH permanently
    $UserPath = [Environment]::GetEnvironmentVariable('PATH', 'User')
    if ($UserPath -notlike "*$InstallDir*") {
        if ($UserPath) {
            [Environment]::SetEnvironmentVariable('PATH', "$InstallDir;$UserPath", 'User')
        } else {
            [Environment]::SetEnvironmentVariable('PATH', "$InstallDir", 'User')
        }
        # Also update current session
        $env:PATH = "$InstallDir;$env:PATH"
        Write-Host "Added $InstallDir to user PATH"
    }
} finally {
    Remove-Item -Path $TmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
