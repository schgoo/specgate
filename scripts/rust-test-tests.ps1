$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$fixture = Join-Path ([IO.Path]::GetTempPath()) "sg-rust-test-$([guid]::NewGuid().ToString('N'))"
$scripts = Join-Path $fixture 'scripts'
New-Item -ItemType Directory -Path $scripts, (Join-Path $fixture 'rust') | Out-Null
$wrapper = Join-Path $scripts 'rust-test.ps1'
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'rust-test.ps1') -Destination $wrapper
$previousBuildDir = $env:CARGO_BUILD_BUILD_DIR
$previousTargetDir = $env:CARGO_TARGET_DIR
$previousThreads = $env:RUST_TEST_THREADS
$callerLocation = (Get-Location).Path
$state = @{ Scratch = $null; Calls = 0; Failure = '' }

function cargo {
    $state.Calls++
    $state.Scratch = $scratch
    $expected = @('test', '--workspace', '--', '--test-threads=1')
    if ($args.Count -ne $expected.Count -or ($args -join "`n") -cne ($expected -join "`n")) {
        throw "Runner changed Cargo workspace, target, doctest, or serial execution selection: $($args -join ' | ')"
    }
    if ((Get-Location).Path -cne (Join-Path $fixture 'rust')) {
        throw 'Runner did not execute Cargo from the product workspace.'
    }
    if ($env:CARGO_BUILD_BUILD_DIR -cne (Join-Path $scratch 'build') -or
        $env:CARGO_TARGET_DIR -cne 'caller-target-dir' -or $env:RUST_TEST_THREADS -cne '8') {
        throw 'Runner did not isolate intermediates while preserving caller target/thread settings.'
    }
    New-Item -ItemType Directory -Path $env:CARGO_BUILD_BUILD_DIR | Out-Null
    [IO.File]::WriteAllText((Join-Path $env:CARGO_BUILD_BUILD_DIR 'intermediate'), 'test')
    if ($state.Failure -eq 'start') { throw 'Cargo could not start.' }
    $global:LASTEXITCODE = if ($state.Failure -eq 'exit') { 23 } else { 0 }
}

function Assert-Run {
    param([string]$Failure = '', [string]$ExpectedError = '', $CallerBuildDir = 'caller-build-dir')

    $state.Calls = 0
    $state.Failure = $Failure
    $env:CARGO_BUILD_BUILD_DIR = $CallerBuildDir
    $caught = $null
    try {
        & $wrapper
    }
    catch {
        $caught = $_.Exception.Message
    }
    if ($ExpectedError -eq '') {
        if ($null -ne $caught) { throw $caught }
    }
    elseif ($null -eq $caught -or -not $caught.Contains($ExpectedError)) {
        throw "Expected runner failure '$ExpectedError', got '$caught'."
    }
    if ($state.Calls -ne 1 -or $env:CARGO_BUILD_BUILD_DIR -cne $CallerBuildDir -or
        (Get-Location).Path -cne $callerLocation -or (Test-Path -LiteralPath $state.Scratch)) {
        throw 'Runner did not run once, restore the caller environment/location, or clean scratch.'
    }
}

try {
    $env:CARGO_TARGET_DIR = 'caller-target-dir'
    $env:RUST_TEST_THREADS = '8'
    Assert-Run
    $firstScratch = $state.Scratch
    Assert-Run
    if ($state.Scratch -ceq $firstScratch) {
        throw 'Separate test invocations reused mutable compilation intermediates.'
    }
    Assert-Run -CallerBuildDir $null
    Assert-Run -CallerBuildDir ''
    Assert-Run -Failure exit -ExpectedError 'Rust workspace tests failed.'
    Assert-Run -Failure start -ExpectedError 'Cargo could not start.'
}
finally {
    $env:CARGO_BUILD_BUILD_DIR = $previousBuildDir
    $env:CARGO_TARGET_DIR = $previousTargetDir
    $env:RUST_TEST_THREADS = $previousThreads
    Remove-Item -LiteralPath $fixture -Recurse -Force
}

Write-Output 'Rust test runner regressions passed.'
