#Requires -Version 5.1
<#
.SYNOPSIS
    Deploy BioShock Head Tracking mod to game directory.

.DESCRIPTION
    Copies the built DLL to the BioShock Remastered game directory,
    backing up the original xinput1_3.dll if present.

.PARAMETER Configuration
    Build configuration to deploy: Debug or Release

.EXAMPLE
    .\deploy.ps1 -Configuration Release
#>

param(
    [Parameter()]
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Debug'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Import shared game detection
. (Join-Path $PSScriptRoot 'detect-game.ps1')

# Constants
$DLL_NAME = 'xinput1_3.dll'
$BACKUP_SUFFIX = '.backup'

function Test-GameInstallation {
    <#
    .SYNOPSIS
        Validate game installation is complete and writable.
    #>
    param(
        [Parameter(Mandatory)]
        [string]$GamePath
    )

    # Check directory exists
    if (-not (Test-Path $GamePath -PathType Container)) {
        return @{
            Success = $false
            Message = 'Game directory does not exist'
        }
    }

    # Check for game executable
    $exePath = Join-Path $GamePath 'BioshockHD.exe'
    if (-not (Test-Path $exePath)) {
        return @{
            Success = $false
            Message = 'BioshockHD.exe not found in game directory'
        }
    }

    # Check write permissions
    try {
        $testFile = Join-Path $GamePath '.deploy_test'
        [void](New-Item -Path $testFile -ItemType File -Force)
        Remove-Item $testFile -Force
    } catch {
        return @{
            Success = $false
            Message = 'Cannot write to game directory - permission denied'
        }
    }

    return @{
        Success = $true
        Message = 'Game installation validated'
    }
}

function Deploy-Mod {
    <#
    .SYNOPSIS
        Deploy the mod DLL to game directory.
    #>

    Write-Host 'BioShock Head Tracking - Deploy Script' -ForegroundColor Cyan
    Write-Host '=======================================' -ForegroundColor Cyan
    Write-Host ''

    # Find game installation
    Write-Host 'Locating game installation...' -ForegroundColor Yellow
    $gamePath = Find-BioshockPath

    if (-not $gamePath) {
        Write-Host ''
        Write-Host 'ERROR: BioShock Remastered installation not found.' -ForegroundColor Red
        Write-Host ''
        Write-Host 'To fix this:' -ForegroundColor Yellow
        Write-Host '1. Verify BioShock Remastered is installed via Steam (AppID: 409710)'
        Write-Host '2. Launch Steam and check your library for "BioShock Remastered"'
        Write-Host '3. If installed to a custom location, set BIOSHOCK_PATH environment variable'
        Write-Host '4. Run ''pixi run deploy'' again'
        exit 1
    }

    Write-Host "Found game at: $gamePath" -ForegroundColor Green

    # Validate installation
    Write-Host 'Validating installation...' -ForegroundColor Yellow
    $validation = Test-GameInstallation -GamePath $gamePath

    if (-not $validation.Success) {
        Write-Host ''
        Write-Host "ERROR: $($validation.Message)" -ForegroundColor Red
        Write-Host ''

        switch -Wildcard ($validation.Message) {
            '*BioshockHD.exe*' {
                Write-Host 'To fix this:' -ForegroundColor Yellow
                Write-Host '1. Verify game installation completed successfully'
                Write-Host '2. In Steam, right-click BioShock Remastered -> Properties -> Local Files -> Verify integrity'
                Write-Host '3. Run ''pixi run deploy'' again after verification completes'
            }
            '*permission*' {
                Write-Host 'To fix this:' -ForegroundColor Yellow
                Write-Host '1. Close any running instances of BioShock Remastered'
                Write-Host '2. Check that no antivirus is blocking write access'
                Write-Host '3. Run PowerShell/Terminal as Administrator'
                Write-Host '4. Run ''pixi run deploy'' again'
            }
        }
        exit 1
    }

    Write-Host 'Installation validated' -ForegroundColor Green

    # Determine source DLL path (32-bit build for the 32-bit game)
    $configFolder = if ($Configuration -eq 'Release') { 'release' } else { 'debug' }
    $sourceDll = Join-Path $PSScriptRoot "..\target\i686-pc-windows-msvc\$configFolder\bioshock_headtrack.dll"

    if (-not (Test-Path $sourceDll)) {
        Write-Host ''
        Write-Host "ERROR: Built DLL not found at: $sourceDll" -ForegroundColor Red
        Write-Host ''
        Write-Host 'To fix this:' -ForegroundColor Yellow
        Write-Host "1. Run 'pixi run build' (or 'pixi run build-release' for release)"
        Write-Host '2. Run ''pixi run deploy'' again'
        exit 1
    }

    $destDll = Join-Path $gamePath $DLL_NAME
    $backupDll = "$destDll$BACKUP_SUFFIX"

    # Backup original DLL if it exists and backup doesn't exist
    if ((Test-Path $destDll) -and -not (Test-Path $backupDll)) {
        Write-Host "Backing up original $DLL_NAME..." -ForegroundColor Yellow
        Copy-Item $destDll $backupDll -Force
        Write-Host "Backup created: $backupDll" -ForegroundColor Green
    }

    # Deploy mod DLL
    Write-Host "Deploying mod ($Configuration configuration)..." -ForegroundColor Yellow
    Copy-Item $sourceDll $destDll -Force

    # Verify deployment
    $sourceSize = (Get-Item $sourceDll).Length
    $destSize = (Get-Item $destDll).Length

    if ($sourceSize -ne $destSize) {
        Write-Host ''
        Write-Host 'ERROR: Deployment verification failed - file sizes do not match' -ForegroundColor Red
        exit 1
    }

    Write-Host ''
    Write-Host 'Deployment successful!' -ForegroundColor Green
    Write-Host "Installed to: $destDll" -ForegroundColor Cyan
    Write-Host ''
    Write-Host 'Usage:' -ForegroundColor Yellow
    Write-Host '1. Start OpenTrack with UDP output on port 4242'
    Write-Host '2. Launch BioShock Remastered, get in-game'
    Write-Host '3. Move camera around to generate log data'
    Write-Host '4. Exit game and check bioshock_headtrack.log for analysis'
    Write-Host ''
    Write-Host 'Hotkeys:' -ForegroundColor Yellow
    Write-Host '  Home   - Recenter view'
    Write-Host '  End    - Toggle tracking ON/OFF'
    Write-Host '  PageUp - Toggle 6DOF position tracking'
    Write-Host '  Chord alternatives: Ctrl+Shift+T/Y/G'
}

# Run deployment
Deploy-Mod
