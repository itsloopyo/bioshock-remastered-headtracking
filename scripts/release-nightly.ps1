[CmdletBinding()]
param([switch]$AllowDirty)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ProjectRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
Import-Module (Join-Path $ProjectRoot 'cameraunlock-core\powershell\NightlyRelease.psm1') -Force

$cargoPath = Join-Path $ProjectRoot 'Cargo.toml'
$cargoContent = Get-Content $cargoPath -Raw
if ($cargoContent -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
    throw "Could not read version from Cargo.toml"
}
$version = $Matches[1]

Publish-NightlyBuild `
    -ModId 'bioshock-remastered' `
    -ModName 'BioshockRemasteredHeadTracking' `
    -Version $version `
    -ProjectRoot $ProjectRoot `
    -AllowDirty:$AllowDirty
