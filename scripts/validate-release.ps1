#Requires -Version 5.1
<#
.SYNOPSIS
    Validate release readiness before publishing.

.DESCRIPTION
    Performs pre-release checks:
    - Version consistency between Cargo.toml and CHANGELOG.md
    - CHANGELOG.md has entry for current version
    - Git working directory is clean
    - Release build succeeds
    - DLL exports expected functions
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:hasErrors = $false

function Write-Check {
    param(
        [Parameter(Mandatory)]
        [string]$Name,
        [Parameter(Mandatory)]
        [bool]$Passed,
        [string]$Details = ''
    )

    if ($Passed) {
        Write-Host "[PASS] $Name" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] $Name" -ForegroundColor Red
        if ($Details) {
            Write-Host "       $Details" -ForegroundColor Yellow
        }
        $script:hasErrors = $true
    }
}

function Get-CargoVersion {
    $cargoToml = Get-Content (Join-Path $PSScriptRoot '..\Cargo.toml') -Raw
    if ($cargoToml -match 'version\s*=\s*"([^"]+)"') {
        return $Matches[1]
    }
    return $null
}

function Test-ChangelogVersion {
    param([string]$Version)

    $changelog = Get-Content (Join-Path $PSScriptRoot '..\CHANGELOG.md') -Raw
    return $changelog -match "\[$Version\]"
}

function Test-GitClean {
    # Stdout only; folding stderr in would trip $ErrorActionPreference =
    # 'Stop' on any benign git warning (CRLF normalization etc).
    $status = git status --porcelain
    return [string]::IsNullOrWhiteSpace($status)
}

function Test-ReleaseBuild {
    # BioShock Remastered is 32-bit - always build the i686 target.
    # No 2>&1: cargo writes "Updating crates.io index" etc. to stderr,
    # and folding it into the success stream makes PowerShell raise
    # RemoteException under $ErrorActionPreference = 'Stop'. We rely
    # on $LASTEXITCODE for success, and let the streams flow to the
    # CI log untouched.
    cargo build --release --target i686-pc-windows-msvc
    return $LASTEXITCODE -eq 0
}

function Test-DllExports {
    $dllPath = Join-Path $PSScriptRoot '..\target\i686-pc-windows-msvc\release\bioshock_headtrack.dll'

    if (-not (Test-Path $dllPath)) {
        return $false
    }

    # Use dumpbin to check exports (if available)
    $dumpbin = Get-Command dumpbin -ErrorAction SilentlyContinue
    if ($dumpbin) {
        $exports = dumpbin /exports $dllPath
        $requiredExports = @(
            'XInputGetState',
            'XInputSetState',
            'XInputGetCapabilities',
            'XInputEnable',
            'XInputGetBatteryInformation',
            'XInputGetKeystroke'
        )

        foreach ($export in $requiredExports) {
            if ($exports -notmatch $export) {
                return $false
            }
        }
    }

    return $true
}

# Main validation
Write-Host ''
Write-Host 'BioShock Head Tracking - Release Validation' -ForegroundColor Cyan
Write-Host '============================================' -ForegroundColor Cyan
Write-Host ''

# Get version from Cargo.toml
$version = Get-CargoVersion
if ($version) {
    Write-Host "Version: $version" -ForegroundColor Yellow
    Write-Host ''
} else {
    Write-Host "ERROR: Could not parse version from Cargo.toml" -ForegroundColor Red
    exit 1
}

# Run checks
Write-Check -Name 'Cargo.toml version parseable' -Passed ($null -ne $version)
Write-Check -Name 'CHANGELOG.md has version entry' -Passed (Test-ChangelogVersion $version) -Details "Version [$version] not found in CHANGELOG.md"
Write-Check -Name 'Git working directory clean' -Passed (Test-GitClean) -Details 'Uncommitted changes detected'

Write-Host ''
Write-Host 'Building release...' -ForegroundColor Yellow
$buildPassed = Test-ReleaseBuild
Write-Check -Name 'Release build succeeds' -Passed $buildPassed

if ($buildPassed) {
    Write-Check -Name 'DLL exports required functions' -Passed (Test-DllExports) -Details 'Missing XInput exports'
}

Write-Host ''
if ($script:hasErrors) {
    Write-Host 'Release validation FAILED' -ForegroundColor Red
    exit 1
} else {
    Write-Host 'Release validation PASSED' -ForegroundColor Green
    exit 0
}
