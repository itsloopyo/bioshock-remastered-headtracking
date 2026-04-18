#Requires -Version 5.1
<#
.SYNOPSIS
    Detect BioShock Remastered installation directory.

.DESCRIPTION
    Searches for BioShock Remastered in common Steam locations and returns the game path.
    This script is designed to be dot-sourced by other scripts.

    Search order:
    1. BIOSHOCK_PATH environment variable
    2. Default Steam installation path
    3. Steam registry + main library
    4. Steam additional library folders (libraryfolders.vdf)
    5. Common Steam library drive locations

.OUTPUTS
    Returns the game path string if found, $null otherwise.
    When run directly, outputs to console and sets exit code.

.EXAMPLE
    # Dot-source for use in other scripts
    . .\detect-game.ps1
    $gamePath = Find-BioshockPath
    if ($gamePath) { Write-Host "Found: $gamePath" }

.EXAMPLE
    # Run directly to check installation
    .\detect-game.ps1
#>

Set-StrictMode -Version Latest

$script:STEAM_DEFAULT_PATH = 'C:\Program Files (x86)\Steam\steamapps\common\BioShock Remastered\Build\Final'

$script:SearchPaths = @(
    'C:\Program Files (x86)\Steam\steamapps\common\BioShock Remastered\Build\Final',
    'C:\Program Files\Steam\steamapps\common\BioShock Remastered\Build\Final',
    'D:\Steam\steamapps\common\BioShock Remastered\Build\Final',
    'D:\SteamLibrary\steamapps\common\BioShock Remastered\Build\Final',
    'E:\Steam\steamapps\common\BioShock Remastered\Build\Final',
    'E:\SteamLibrary\steamapps\common\BioShock Remastered\Build\Final',
    'F:\Steam\steamapps\common\BioShock Remastered\Build\Final',
    'F:\SteamLibrary\steamapps\common\BioShock Remastered\Build\Final',
    'G:\Steam\steamapps\common\BioShock Remastered\Build\Final',
    'G:\SteamLibrary\steamapps\common\BioShock Remastered\Build\Final'
)

function Test-BioshockPath {
    <#
    .SYNOPSIS
        Test if a path contains a valid BioShock Remastered installation.
    #>
    param(
        [Parameter(Mandatory)]
        [string]$Path
    )

    if (-not (Test-Path $Path -PathType Container)) {
        return $false
    }

    $exePath = Join-Path $Path 'BioshockHD.exe'
    return (Test-Path $exePath -PathType Leaf)
}

function Find-BioshockPath {
    <#
    .SYNOPSIS
        Locate BioShock Remastered installation directory.

    .OUTPUTS
        String path if found, $null otherwise.
    #>

    # 1. Check environment variable override
    if ($env:BIOSHOCK_PATH) {
        if (Test-BioshockPath $env:BIOSHOCK_PATH) {
            return $env:BIOSHOCK_PATH
        }
    }

    # 2. Check default Steam path
    if (Test-BioshockPath $script:STEAM_DEFAULT_PATH) {
        return $script:STEAM_DEFAULT_PATH
    }

    # 3. Query Steam registry for install path
    $steamPath = $null
    try {
        $steamPath = (Get-ItemProperty -Path 'HKLM:\SOFTWARE\WOW6432Node\Valve\Steam' -Name 'InstallPath' -ErrorAction SilentlyContinue).InstallPath
    } catch {
        # Registry key not found
    }

    if ($steamPath) {
        # Check main Steam library
        $mainLibrary = Join-Path $steamPath 'steamapps\common\BioShock Remastered\Build\Final'
        if (Test-BioshockPath $mainLibrary) {
            return $mainLibrary
        }

        # 4. Check additional library folders from libraryfolders.vdf
        $libraryFoldersPath = Join-Path $steamPath 'steamapps\libraryfolders.vdf'
        if (Test-Path $libraryFoldersPath) {
            $content = Get-Content $libraryFoldersPath -Raw
            $vdfMatches = [regex]::Matches($content, '"path"\s+"([^"]+)"')
            foreach ($match in $vdfMatches) {
                $libraryPath = $match.Groups[1].Value
                $gamePath = Join-Path $libraryPath 'steamapps\common\BioShock Remastered\Build\Final'
                if (Test-BioshockPath $gamePath) {
                    return $gamePath
                }
            }
        }
    }

    # 5. Check common library drive locations
    foreach ($path in $script:SearchPaths) {
        try {
            if (Test-BioshockPath $path) {
                return $path
            }
        } catch {
            # Drive doesn't exist or access denied, skip
        }
    }

    return $null
}

# If run directly (not dot-sourced), output result
if ($MyInvocation.InvocationName -ne '.') {
    $gamePath = Find-BioshockPath

    if ($gamePath) {
        Write-Host "BioShock Remastered found at:" -ForegroundColor Green
        Write-Host "  $gamePath" -ForegroundColor Cyan
        exit 0
    } else {
        Write-Host "BioShock Remastered not found" -ForegroundColor Red
        Write-Host ""
        Write-Host "Checked locations:" -ForegroundColor Yellow
        Write-Host "  - BIOSHOCK_PATH environment variable"
        Write-Host "  - Default Steam path: $script:STEAM_DEFAULT_PATH"
        Write-Host "  - Steam registry + library folders"
        Write-Host "  - Common library drives (D:, E:, F:, G:)"
        Write-Host ""
        Write-Host "To specify a custom path, set BIOSHOCK_PATH environment variable" -ForegroundColor Gray
        exit 1
    }
}
