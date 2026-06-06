#Requires -Version 5.1
# Thin wrapper - detection lives in cameraunlock-core/powershell/GamePathDetection.psm1
# (games.json id: bioshock-remastered).

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Split-Path -Parent $scriptDir

Import-Module (Join-Path $projectRoot 'cameraunlock-core\powershell\GamePathDetection.psm1') -Force

$gamePath = Find-GamePath -GameId 'bioshock-remastered'

if ($gamePath) {
    Write-Host 'BioShock Remastered found at:' -ForegroundColor Green
    Write-Host "  $gamePath" -ForegroundColor Cyan
    Write-Host "  exe dir: $(Join-Path $gamePath 'Build\Final')" -ForegroundColor Cyan
    exit 0
}

Write-GameNotFoundError -GameName 'BioShock Remastered' -EnvVar 'BIOSHOCK_PATH' -SteamFolder 'BioShock Remastered'
exit 1
