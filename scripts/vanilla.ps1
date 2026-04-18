#Requires -Version 5.1
<#
.SYNOPSIS
    Restore BioShock Remastered to vanilla (unmodded) state.

.DESCRIPTION
    Removes the mod and restores the original xinput1_3.dll from backup.
    If no backup exists, removes the mod DLL (game may need Steam verification).

.EXAMPLE
    .\vanilla.ps1
    pixi run vanilla
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Import shared game detection
. (Join-Path $PSScriptRoot 'detect-game.ps1')

$DLL_NAME = 'xinput1_3.dll'
$BACKUP_SUFFIX = '.backup'
$StateFileName = '.headtracking-state.json'

Write-Host ''
Write-Host 'BioShock Head Tracking - Restore Vanilla' -ForegroundColor Cyan
Write-Host '=========================================' -ForegroundColor Cyan
Write-Host ''

$gamePath = Find-BioshockPath

if (-not $gamePath) {
    Write-Host 'ERROR: BioShock Remastered not found' -ForegroundColor Red
    exit 1
}

Write-Host "Found game at: $gamePath" -ForegroundColor Green
Write-Host ''

$dllPath = Join-Path $gamePath $DLL_NAME
$backupPath = "$dllPath$BACKUP_SUFFIX"
$stateFile = Join-Path $gamePath $StateFileName
$restored = $false

# Restore from backup if it exists
if (Test-Path $backupPath) {
    Write-Host "Restoring original $DLL_NAME from backup..." -ForegroundColor Yellow

    # Remove current DLL (mod)
    if (Test-Path $dllPath) {
        Remove-Item $dllPath -Force
    }

    # Restore backup
    Move-Item $backupPath $dllPath -Force
    Write-Host "  Restored: $DLL_NAME" -ForegroundColor Green
    $restored = $true
} else {
    # No backup - just remove mod DLL
    if (Test-Path $dllPath) {
        Remove-Item $dllPath -Force
        Write-Host "  Removed: $DLL_NAME (no backup available)" -ForegroundColor Yellow
        Write-Host ''
        Write-Host 'Warning: No backup was found to restore.' -ForegroundColor Yellow
        Write-Host 'Use Steam to verify game files:' -ForegroundColor Gray
        Write-Host '  1. Open Steam Library' -ForegroundColor Gray
        Write-Host '  2. Right-click BioShock Remastered' -ForegroundColor Gray
        Write-Host '  3. Properties -> Local Files -> Verify integrity' -ForegroundColor Gray
        $restored = $true
    }
}

# Remove state file
if (Test-Path $stateFile) {
    Remove-Item $stateFile -Force
    Write-Host "  Removed: $StateFileName" -ForegroundColor Gray
}

Write-Host ''
if ($restored) {
    Write-Host 'Game is now vanilla (unmodded)' -ForegroundColor Green
} else {
    Write-Host 'No mod files found - game is already vanilla' -ForegroundColor Yellow
}
Write-Host ''
