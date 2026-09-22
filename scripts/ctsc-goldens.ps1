<#
.SYNOPSIS
    Regenerate or verify the CTSC golden matrix artifacts.

.DESCRIPTION
    `Update` rewrites every generated artifact under test/goldens/ctsc.
    `Check` regenerates into repository-local scratch, validates the fresh
    output with the native CTSC validators, replays the components the matrix
    marks replayable, and byte-compares everything against the checked-in
    goldens. The matrix itself is hand-authored configuration; nothing else
    under test/goldens/ctsc may be edited by hand.
#>
[CmdletBinding()]
param(
    [ValidateSet('Update', 'Check')]
    [string]$Mode = 'Check'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $root 'rust/Cargo.toml'
$harnessTest = 'goldens::ctsc_golden_matrix'
. (Join-Path $PSScriptRoot 'ctsc-goldens-support.ps1')
# Scratch is invocation-unique so a concurrent check and update never share,
# and therefore never clobber, the same regeneration directory.
$scratch = Join-Path $root "rust/target/ctsc-goldens-$($Mode.ToLowerInvariant())-$PID"
$cache = Join-Path $scratch 'cache'

$previousCache = $env:SPECGATE_CACHE_DIR
$previousMode = $env:SPECGATE_CTSC_GOLDENS
$previousScratch = $env:SPECGATE_CTSC_GOLDENS_SCRATCH

try {
    New-Item -ItemType Directory -Force $cache | Out-Null
    $env:SPECGATE_CACHE_DIR = $cache
    $env:SPECGATE_CTSC_GOLDENS = $Mode.ToLowerInvariant()
    $env:SPECGATE_CTSC_GOLDENS_SCRATCH = $scratch

    $buildOutput = @(
        & cargo test --manifest-path $manifest -p specgate-cli --lib --no-run --quiet --message-format=json 2>&1
    )
    if ($LASTEXITCODE -ne 0) {
        $buildOutput | ForEach-Object { Write-Host $_ }
        throw 'Failed to build the CTSC golden harness.'
    }
    $harness = Get-GoldenHarnessExecutable -CargoMessages $buildOutput

    $listOutput = @(& $harness --ignored --exact --list $harnessTest 2>&1)
    if ($LASTEXITCODE -ne 0) {
        $listOutput | ForEach-Object { Write-Host $_ }
        throw 'Failed to enumerate the CTSC golden harness test.'
    }
    Assert-SingleGoldenTestSelection -Lines $listOutput -TestName $harnessTest

    & $harness --ignored --exact --nocapture $harnessTest 2>&1 |
        Tee-Object -Variable executionOutput
    $executionExitCode = $LASTEXITCODE
    if ($executionExitCode -ne 0) {
        if ($Mode -eq 'Check') {
            throw 'CTSC golden check failed. Review the failure above; if the change is intended, run `just ctsc-goldens-update` and review the artifact diff.'
        }
        throw 'CTSC golden update failed.'
    }
    Assert-SingleGoldenTestExecution -Lines @($executionOutput)
}
finally {
    $env:SPECGATE_CACHE_DIR = $previousCache
    $env:SPECGATE_CTSC_GOLDENS = $previousMode
    $env:SPECGATE_CTSC_GOLDENS_SCRATCH = $previousScratch
    Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
}
