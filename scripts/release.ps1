#!/usr/bin/env pwsh
#Requires -Version 5.1
<#
.SYNOPSIS
    Release workflow for BioShock Remastered Head Tracking.

.DESCRIPTION
    1. Validate semver + git state.
    2. Bump version in Cargo.toml.
    3. Build the i686-pc-windows-msvc release.
    4. Regenerate CHANGELOG.md from conventional commits (via
       cameraunlock-core/powershell/ReleaseWorkflow.psm1).
    5. Commit the version + changelog as "Release v<version>".
    6. Create annotated tag v<version> and push it; CI picks up the tag and
       produces the GitHub release artifacts.

.PARAMETER Version
    Semver string (e.g. "1.0.0"). Required.

.EXAMPLE
    pixi run release 1.0.0
#>
param(
    [Parameter(Position = 0)]
    [string]$Version = '',
    # Ship a release even when there are no user-facing commits since the
    # last tag (writes a maintenance changelog entry instead of aborting).
    [switch]$Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectDir = Split-Path -Parent $scriptDir
$cargoPath = Join-Path $projectDir 'Cargo.toml'
$cargoLockPath = Join-Path $projectDir 'Cargo.lock'
$changelogPath = Join-Path $projectDir 'CHANGELOG.md'

Import-Module (Join-Path $projectDir 'cameraunlock-core\powershell\ReleaseWorkflow.psm1') -Force

function Get-CargoVersion {
    $content = Get-Content $cargoPath -Raw
    if ($content -match '(?m)^version\s*=\s*"([^"]+)"') {
        return $Matches[1]
    }
    throw "Could not read version from Cargo.toml"
}

function Set-CargoVersion {
    param([string]$NewVersion)
    $content = Get-Content $cargoPath -Raw
    # Replace ONLY the first `version = "..."` line (the [package]
    # version). The naive `-replace` operator clobbers every match,
    # which corrupts dependency versions like the one in
    # [dependencies.windows].
    $rx = [regex]::new('(?m)^(version\s*=\s*)"[^"]+"')
    $content = $rx.Replace($content, "`${1}`"$NewVersion`"", 1)
    Set-Content -Path $cargoPath -Value $content -NoNewline
}

# Mirrors New-ChangelogFromCommits' insertion so a -Force maintenance entry
# lands in the same place with the same shape.
function Add-MaintenanceChangelogEntry {
    param([string]$Path, [string]$NewVersion)
    $date = Get-Date -Format 'yyyy-MM-dd'
    $entry = "## [$NewVersion] - $date`n`n### Changed`n`n- Maintenance release (no user-facing changes).`n`n"
    $changelog = Get-Content $Path -Raw
    if ($changelog -match '(?s)(# Changelog.*?)(## \[)') {
        $changelog = $changelog -replace '(?s)(# Changelog.*?\n\n)', "`$1$entry"
    } else {
        $changelog = $changelog -replace '(?s)(# Changelog.*?\n)', "`$1$entry"
    }
    $changelog = $changelog.TrimEnd() + "`n"
    Set-Content $Path $changelog -NoNewline
}

Write-Host ''
Write-Host '=== BioShock Remastered Head Tracking Release ===' -ForegroundColor Cyan
Write-Host ''

$current = Get-CargoVersion

if ([string]::IsNullOrWhiteSpace($Version)) {
    Write-Host "Current version: $current" -ForegroundColor Yellow
    Write-Host 'Usage: pixi run release <major|minor|patch|nightly|X.Y.Z>'
    exit 0
}

if ($Version -eq 'nightly') {
    & (Join-Path $scriptDir 'release-nightly.ps1')
    exit $LASTEXITCODE
}

try {
    $Version = Resolve-ReleaseVersion -Argument $Version -CurrentVersion $current
} catch {
    Write-Host "Error: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}

$tag = "v$Version"

$branch = git rev-parse --abbrev-ref HEAD
if ($branch -ne 'main') {
    Write-Host "Must be on main branch to release (currently on '$branch')" -ForegroundColor Red
    exit 1
}
if (-not (Test-CleanGitStatus)) {
    Write-Host 'Working tree has uncommitted changes - commit or stash first.' -ForegroundColor Red
    git status --short
    exit 1
}
if (Test-GitTagExists -Tag $tag) {
    Write-Host "Tag '$tag' already exists." -ForegroundColor Red
    exit 1
}

Write-Host "Current version: $current" -ForegroundColor Gray
Write-Host "New version:     $Version" -ForegroundColor Green
Write-Host ''

# Step 1 - changelog (the gate that can fail). Generate it BEFORE mutating
# any version files so an abort here leaves the working tree clean instead
# of stranding a half-applied version bump with no tag.
Write-Host 'Generating CHANGELOG from commits...' -ForegroundColor Cyan
$hasTags = git tag -l 2>$null
if (-not $hasTags) {
    $date = Get-Date -Format 'yyyy-MM-dd'
    Set-Content $changelogPath "# Changelog`n`n## [$Version] - $date`n`nFirst release.`n"
} else {
    try {
        $changelogArgs = @{
            ChangelogPath = $changelogPath
            Version       = $Version
            ArtifactPaths = @('src/', 'cameraunlock-core', 'scripts/')
        }
        New-ChangelogFromCommits @changelogArgs
    } catch {
        if (-not $Force) {
            Write-Host "Error: $($_.Exception.Message)" -ForegroundColor Red
            Write-Host 'No user-facing changes to release. Re-run with -Force for a maintenance release.' -ForegroundColor Yellow
            exit 1
        }
        Write-Host 'No user-facing commits since last tag - writing maintenance entry (-Force).' -ForegroundColor Yellow
        Add-MaintenanceChangelogEntry -Path $changelogPath -NewVersion $Version
    }
}

# Step 2 - bump version in Cargo.toml + install.cmd
Write-Host "Updating Cargo.toml to $Version..." -ForegroundColor Cyan
Set-CargoVersion -NewVersion $Version

# Keep install.cmd's MOD_VERSION in lockstep - it's what the installer
# writes into the user's .headtracking-state.json. ReadAllText/WriteAllText
# preserve the file's CRLF line endings.
$installCmdPath = Join-Path $projectDir 'scripts\install.cmd'
$installRaw = [System.IO.File]::ReadAllText($installCmdPath)
if ($installRaw -notmatch 'set "MOD_VERSION=[^"]+"') {
    throw "MOD_VERSION line not found in $installCmdPath"
}
$installRaw = [regex]::Replace($installRaw, 'set "MOD_VERSION=[^"]+"', "set `"MOD_VERSION=$Version`"")
[System.IO.File]::WriteAllText($installCmdPath, $installRaw)
Write-Host "Updating scripts/install.cmd MOD_VERSION to $Version..." -ForegroundColor Cyan

# Step 3 - build (refreshes Cargo.lock's version entry)
Write-Host 'Building release (i686-pc-windows-msvc)...' -ForegroundColor Cyan
Push-Location $projectDir
try {
    pixi run build-release
    if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
} finally {
    Pop-Location
}

# Step 4 - commit specific files only (avoid git add -A sweeping in build artifacts)
Write-Host 'Committing version + changelog...' -ForegroundColor Cyan
git add $cargoPath $cargoLockPath $changelogPath $installCmdPath
# Skip commit if everything is already at this version (re-running release
# for the same version, e.g. after deleting a tag / release on GitHub to
# republish). The tag still gets recreated below at the current HEAD.
git diff --cached --quiet
if ($LASTEXITCODE -eq 0) {
    Write-Host 'No version/changelog changes - tagging existing HEAD.' -ForegroundColor Yellow
} else {
    git commit -m "Release v$Version"
    if ($LASTEXITCODE -ne 0) { throw 'Commit failed' }
}

# Step 5 - tag + push
Write-Host "Creating tag $tag..." -ForegroundColor Cyan
git tag -a $tag -m "Release $tag"
git push origin main
git push origin $tag

Write-Host ''
Write-Host "Release $tag pushed - CI will build and publish artifacts." -ForegroundColor Green
