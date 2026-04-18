#!/usr/bin/env pwsh
#Requires -Version 5.1
<#
.SYNOPSIS
    Build the release distribution ZIPs for BioShock Remastered Head Tracking.

.DESCRIPTION
    Produces two ZIPs under release/ :

    1. BioshockRemasteredHeadTracking-v<ver>-installer.zip
         install.cmd, uninstall.cmd, plugins/xinput1_3.dll, README.md,
         LICENSE, CHANGELOG.md, THIRD_PARTY_LICENSES.md.
         Users run install.cmd - it locates the game and deploys the DLL.

    2. BioshockRemasteredHeadTracking-v<ver>-nexus.zip
         Build/Final/xinput1_3.dll
         NexusMods-compatible: extract directly into the game folder.

    Assumes `pixi run build-release` has already produced the 32-bit DLL.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Split-Path -Parent $scriptDir
$releaseDir = Join-Path $projectRoot 'release'

$cargoPath = Join-Path $projectRoot 'Cargo.toml'
$cargoToml = Get-Content $cargoPath -Raw
if ($cargoToml -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
    throw "Could not parse version from Cargo.toml"
}
$version = $Matches[1]

$modName = 'BioshockRemasteredHeadTracking'
$builtDll = Join-Path $projectRoot 'target\i686-pc-windows-msvc\release\bioshock_headtrack.dll'

if (-not (Test-Path $builtDll)) {
    Write-Host "Build output not found: $builtDll" -ForegroundColor Red
    Write-Host "Run 'pixi run build-release' first." -ForegroundColor Yellow
    exit 1
}

Write-Host ''
Write-Host "=== Packaging $modName v$version ===" -ForegroundColor Cyan
Write-Host ''

if (-not (Test-Path $releaseDir)) {
    New-Item -ItemType Directory -Path $releaseDir -Force | Out-Null
}

function New-ZipFromStaging {
    param(
        [Parameter(Mandatory)] [string]$Name,
        [Parameter(Mandatory)] [string]$StagingDir
    )
    $zipPath = Join-Path $releaseDir $Name
    if (Test-Path $zipPath) { Remove-Item $zipPath -Force }

    Push-Location $StagingDir
    try {
        Compress-Archive -Path '.\*' -DestinationPath $zipPath -Force
    } finally {
        Pop-Location
    }

    $kb = [math]::Round((Get-Item $zipPath).Length / 1KB, 1)
    Write-Host "  $Name ($kb KB)" -ForegroundColor Green
    return $zipPath
}

# --- Installer ZIP -----------------------------------------------------
Write-Host 'Staging installer ZIP...' -ForegroundColor Cyan
$installerStaging = Join-Path $releaseDir 'staging-installer'
if (Test-Path $installerStaging) { Remove-Item $installerStaging -Recurse -Force }
New-Item -ItemType Directory -Path $installerStaging -Force | Out-Null

$pluginsDir = Join-Path $installerStaging 'plugins'
New-Item -ItemType Directory -Path $pluginsDir -Force | Out-Null
Copy-Item -Path $builtDll -Destination (Join-Path $pluginsDir 'xinput1_3.dll') -Force

# Mirror the DLL under profile/asi/ for the CameraUnlock Launcher. Its
# AsiLoader strategy expects `<profile>/asi/*` after staging, so the ZIP
# needs this subtree alongside the existing plugins/ layout that install.cmd
# still reads from. Duplicating one ~200KB DLL is cheaper than threading a
# layout change through the battle-tested install.cmd.
$launcherProfileDir = Join-Path (Join-Path $installerStaging 'profile') 'asi'
New-Item -ItemType Directory -Path $launcherProfileDir -Force | Out-Null
Copy-Item -Path $builtDll -Destination (Join-Path $launcherProfileDir 'xinput1_3.dll') -Force

# Stamp launcher-manifest.json with the real release version and drop it
# at the installer ZIP root. The launcher reads this to choose the staging
# + launch path (delivery_mode: profile -> session-isolated copy-in/out).
$manifestSource = Join-Path $projectRoot 'launcher-manifest.json'
if (-not (Test-Path $manifestSource)) {
    throw "launcher-manifest.json not found at repo root ($manifestSource)"
}
$manifestJson = Get-Content $manifestSource -Raw | ConvertFrom-Json
$manifestJson.mod_info.version = $version
# Write UTF-8 without BOM - PowerShell 5.1's -Encoding UTF8 would prefix
# the file with EF BB BF, which serde_json in the launcher rejects with
# "expected value at line 1 column 1".
$utf8NoBom = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText(
    (Join-Path $installerStaging 'launcher-manifest.json'),
    ($manifestJson | ConvertTo-Json -Depth 10),
    $utf8NoBom
)

foreach ($s in @('install.cmd', 'uninstall.cmd')) {
    Copy-Item -Path (Join-Path $scriptDir $s) -Destination $installerStaging -Force
}
foreach ($doc in @('README.md', 'LICENSE', 'CHANGELOG.md', 'THIRD_PARTY_LICENSES.md')) {
    $p = Join-Path $projectRoot $doc
    if (Test-Path $p) { Copy-Item -Path $p -Destination $installerStaging -Force }
}

$installerZip = New-ZipFromStaging -Name "$modName-v$version-installer.zip" -StagingDir $installerStaging
Remove-Item $installerStaging -Recurse -Force

# --- Nexus ZIP ---------------------------------------------------------
Write-Host 'Staging Nexus ZIP...' -ForegroundColor Cyan
$nexusStaging = Join-Path $releaseDir 'staging-nexus'
if (Test-Path $nexusStaging) { Remove-Item $nexusStaging -Recurse -Force }

$nexusDllDir = Join-Path $nexusStaging 'Build\Final'
New-Item -ItemType Directory -Path $nexusDllDir -Force | Out-Null
Copy-Item -Path $builtDll -Destination (Join-Path $nexusDllDir 'xinput1_3.dll') -Force

$nexusZip = New-ZipFromStaging -Name "$modName-v$version-nexus.zip" -StagingDir $nexusStaging
Remove-Item $nexusStaging -Recurse -Force

Write-Host ''
Write-Host 'Done.' -ForegroundColor Green
Write-Host "  installer: $installerZip"
Write-Host "  nexus:     $nexusZip"

# CI picks these up
Write-Output $installerZip
Write-Output $nexusZip
