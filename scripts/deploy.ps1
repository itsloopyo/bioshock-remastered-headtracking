#!/usr/bin/env pwsh
#Requires -Version 5.1
# Thin wrapper - dev-deploy orchestration lives in
# cameraunlock-core/powershell/DevDeploy.psm1.

param(
    [Parameter(Mandatory=$true, Position=0)]
    [ValidateSet("Debug", "Release")]
    [string]$Configuration,
    [Parameter(Mandatory=$false, Position=1)]
    [string]$GivenPath,
    [Parameter(ValueFromRemainingArguments=$true)]
    [string[]]$RemainingArgs
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = 'SilentlyContinue'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Split-Path -Parent $scriptDir

Import-Module (Join-Path $projectRoot "cameraunlock-core\powershell\DevDeploy.psm1") -Force
Import-Module (Join-Path $projectRoot "cameraunlock-core\powershell\ModDeployment.psm1") -Force
# Cargo emits lowercase profile dirs (debug / release), not Debug/Release.
$cargoProfile = $Configuration.ToLower()
$buildOutput = Join-Path $projectRoot "target\i686-pc-windows-msvc\$cargoProfile"
$result = Invoke-DevDeployShim `
    -GameId 'bioshock-remastered' `
    -GameDisplayName 'Bioshock Remastered' `
    -BuildOutputPath $buildOutput `
    -ModDllName 'xinput1_3.dll' `
    -SourceDllName 'bioshock_headtrack.dll' `
    -ExtraDlls @() `
    -GivenPath $GivenPath

Write-DeploymentSuccess `
    -ModName "Head Tracking mod" `
    -DeployPath $result.DeployedDllPath `
    -RecenterKey "Home" `
    -ToggleKey "End"