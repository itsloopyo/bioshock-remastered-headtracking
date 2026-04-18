#Requires -Version 5.1
<#
.SYNOPSIS
    Uninstall BioShock Head Tracking mod (keeps backup).

.DESCRIPTION
    Removes the mod DLL from the game directory but preserves the backup.
    Use vanilla.ps1 to restore the original DLL.

.EXAMPLE
    .\uninstall.ps1
    pixi run uninstall
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Import shared game detection
. (Join-Path $PSScriptRoot 'detect-game.ps1')

$DLL_NAME = 'xinput1_3.dll'
$StateFileName = '.headtracking-state.json'

Write-Host ''
Write-Host 'BioShock Head Tracking - Uninstall' -ForegroundColor Cyan
Write-Host '===================================' -ForegroundColor Cyan
Write-Host ''

$gamePath = Find-BioshockPath

if (-not $gamePath) {
    Write-Host 'BioShock Remastered not found' -ForegroundColor Yellow
    Write-Host 'Nothing to uninstall' -ForegroundColor Gray
    exit 0
}

Write-Host "Found game at: $gamePath" -ForegroundColor Green

$modDll = Join-Path $gamePath $DLL_NAME
$removed = $false

if (Test-Path $modDll) {
    Remove-Item $modDll -Force
    Write-Host "  Removed: $DLL_NAME" -ForegroundColor Green
    $removed = $true
}

# Update state file
$stateFile = Join-Path $gamePath $StateFileName
if (Test-Path $stateFile) {
    try {
        $state = Get-Content $stateFile -Raw | ConvertFrom-Json
        $state.mod_files = @()
        $state | ConvertTo-Json -Depth 10 | Set-Content $stateFile -Encoding UTF8
    } catch {
        Remove-Item $stateFile -Force -ErrorAction SilentlyContinue
    }
}

Write-Host ''
if ($removed) {
    Write-Host 'Uninstall complete!' -ForegroundColor Green
    Write-Host 'Backup preserved for vanilla restore' -ForegroundColor Gray
} else {
    Write-Host 'No mod files found to remove' -ForegroundColor Yellow
}
Write-Host ''
Write-Host 'To restore original DLL: pixi run vanilla' -ForegroundColor Gray
Write-Host ''
