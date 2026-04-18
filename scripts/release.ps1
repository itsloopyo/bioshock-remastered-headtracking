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

.PARAMETER Force
    Skip the branch / clean-tree / tag-exists guards (use sparingly).

.EXAMPLE
    pixi run release 1.0.0
#>
param(
    [Parameter(Position = 0)]
    [string]$Version = '',
    [switch]$Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectDir = Split-Path -Parent $scriptDir
$cargoPath = Join-Path $projectDir 'Cargo.toml'
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

Write-Host ''
Write-Host '=== BioShock Remastered Head Tracking Release ===' -ForegroundColor Cyan
Write-Host ''

if ([string]::IsNullOrWhiteSpace($Version)) {
    Write-Host "Current version: $(Get-CargoVersion)" -ForegroundColor Yellow
    Write-Host 'Usage: pixi run release <version>   (e.g. 1.0.0)'
    exit 0
}

if (-not (Test-SemanticVersion -Version $Version)) {
    Write-Host "Invalid version '$Version'. Use X.Y.Z." -ForegroundColor Red
    exit 1
}

$tag = "v$Version"

if (-not $Force) {
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
}

$current = Get-CargoVersion
Write-Host "Current version: $current" -ForegroundColor Gray
Write-Host "New version:     $Version" -ForegroundColor Green
Write-Host ''

$confirm = Read-Host 'Continue? (y/N)'
if ($confirm -ne 'y' -and $confirm -ne 'Y') {
    Write-Host 'Cancelled.' -ForegroundColor Yellow
    exit 0
}

# Step 1 - bump version
Write-Host "Updating Cargo.toml to $Version..." -ForegroundColor Cyan
Set-CargoVersion -NewVersion $Version

# Step 2 - build
Write-Host 'Building release (i686-pc-windows-msvc)...' -ForegroundColor Cyan
Push-Location $projectDir
try {
    pixi run build-release
    if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
} finally {
    Pop-Location
}

# Step 3 - changelog
Write-Host 'Generating CHANGELOG from commits...' -ForegroundColor Cyan
$hasTags = git tag -l 2>$null
if (-not $hasTags) {
    $date = Get-Date -Format 'yyyy-MM-dd'
    Set-Content $changelogPath "# Changelog`n`n## [$Version] - $date`n`nFirst release.`n"
} else {
    $args = @{
        ChangelogPath = $changelogPath
        Version       = $Version
        ArtifactPaths = @('src/', 'cameraunlock-core', 'scripts/install.cmd', 'scripts/uninstall.cmd')
    }
    if ($Force) { $args.IncludeAll = $true }
    New-ChangelogFromCommits @args
}

# Step 4 - commit specific files only (avoid git add -A sweeping in build artifacts)
Write-Host 'Committing version + changelog...' -ForegroundColor Cyan
git add $cargoPath $changelogPath
# Skip commit if Cargo.toml + CHANGELOG.md are already at this version
# (re-running release for the same version, e.g. after deleting a
# tag / release on GitHub to republish). The tag still gets recreated
# below at the current HEAD.
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
