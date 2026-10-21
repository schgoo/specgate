$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = Split-Path -Parent $PSScriptRoot
$golden = Join-Path $root 'test/goldens/ctsc/component/fixture.stateless_add'
$fixture = Join-Path ([IO.Path]::GetTempPath()) "sg-smoke-tests-$([guid]::NewGuid().ToString('N'))"
$scripts = Join-Path $fixture 'scripts'
New-Item -ItemType Directory -Force $scripts | Out-Null
$wrapper = Join-Path $scripts 'ctsc-smoke.ps1'
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'ctsc-smoke.ps1') -Destination $wrapper
$previousBuildDir = $env:CARGO_BUILD_BUILD_DIR
$state = @{
    Scratch = $null; Builds = 0; Calls = [Collections.Generic.List[object]]::new()
    Failure = ''; ArtifactCount = 1
}

function cargo {
    $state.Scratch = $scratch
    $state.Builds++
    $expected = @('build', '--manifest-path', (Join-Path $fixture 'rust/Cargo.toml'), '-p', 'specgate-cli', '--bin', 'specgate', '--quiet', '--message-format=json')
    if ($env:CARGO_BUILD_BUILD_DIR -cne 'caller-build-dir' -or
        $args.Count -ne $expected.Count -or ($args -join "`n") -cne ($expected -join "`n")) {
        throw 'Smoke did not build the CLI once using the caller compilation environment.'
    }
    $global:LASTEXITCODE = if ($state.Failure -eq 'build') { 23 } else { 0 }
    for ($index = 0; $index -lt $state.ArtifactCount; $index++) {
        @{
            reason = 'compiler-artifact'
            target = @{ name = 'specgate'; kind = @('bin') }
            executable = if ($index -eq 0) { 'Invoke-TestSmokeCli' } else { 'duplicate-specgate' }
        } | ConvertTo-Json -Compress
    }
    foreach ($target in @(
        @{ name = 'specgate'; kind = @('lib') },
        @{ name = 'other'; kind = @('bin') }
    )) {
        @{ reason = 'compiler-artifact'; target = $target; executable = 'not-the-cli' } | ConvertTo-Json -Compress
    }
    @{
        reason = 'compiler-artifact'
        target = @{ name = 'specgate'; kind = @('bin') }
        executable = $null
    } | ConvertTo-Json -Compress
    @{ reason = 'build-finished'; success = ($global:LASTEXITCODE -eq 0) } | ConvertTo-Json -Compress
}

function Invoke-TestSmokeCli {
    $arguments = @($args)
    $state.Calls.Add($arguments)
    if ($env:CARGO_BUILD_BUILD_DIR -cne (Join-Path $state.Scratch 'build')) {
        throw 'Smoke commands did not share invocation-local compilation intermediates.'
    }
    $global:LASTEXITCODE = 0
    if ($state.Failure -eq $arguments[0]) {
        $global:LASTEXITCODE = 23
        return
    }
    switch ($arguments[0]) {
        capture {
            $out = $arguments[[Array]::IndexOf($arguments, '--out') + 1]
            New-Item -ItemType Directory -Force $out | Out-Null
            foreach ($name in @('manifest.json', 'registry.ctsc.json', 'reference.otlp.json')) {
                Copy-Item -LiteralPath (Join-Path $golden $name) -Destination $out
            }
        }
        replay {
            $out = $arguments[[Array]::IndexOf($arguments, '--out') + 1]
            $reference = [IO.File]::ReadAllText((Join-Path $golden 'reference.otlp.json'))
            $traceId = ($reference | ConvertFrom-Json).resourceSpans[0].scopeSpans[0].spans[0].traceId
            [IO.File]::WriteAllText($out, $reference.Replace($traceId, '11111111111111111111111111111111'))
        }
        compare {
            if ((Split-Path -Leaf $arguments[2]) -eq 'candidate-changed.otlp.json') {
                $changed = [IO.File]::ReadAllText($arguments[2])
                if ($changed -notmatch '"intValue":"6"') {
                    throw 'Smoke did not mutate the captured CTSC result for its negative comparison.'
                }
                $global:LASTEXITCODE = if ($state.Failure -eq 'mismatch-exit') { 2 } else { 1 }
                if ($state.Failure -ne 'mismatch-path') {
                    Write-Output 'mismatch scenario[0].operation[0].result'
                }
            }
        }
    }
}

function Assert-SmokeRun {
    param([string]$Failure = '', [int]$Artifacts = 1, [string]$ExpectedError = '')

    $state.Builds = 0
    $state.Calls.Clear()
    $state.Failure = $Failure
    $state.ArtifactCount = $Artifacts
    $caught = $null
    try {
        & $wrapper 6>$null | Out-Null
    }
    catch {
        $caught = $_.Exception.Message
    }
    if ($ExpectedError -eq '') {
        if ($null -ne $caught) { throw $caught }
    }
    elseif ($null -eq $caught -or -not $caught.Contains($ExpectedError)) {
        throw "Expected smoke failure '$ExpectedError', got '$caught'."
    }
    if ($state.Builds -ne 1 -or $env:CARGO_BUILD_BUILD_DIR -cne 'caller-build-dir' -or
        (Test-Path -LiteralPath $state.Scratch)) {
        throw 'Smoke did not build once, restore its caller environment, or clean invocation scratch.'
    }
}

try {
    $env:CARGO_BUILD_BUILD_DIR = 'caller-build-dir'
    Assert-SmokeRun
    $firstScratch = $state.Scratch
    $commands = @($state.Calls | ForEach-Object { $_[0] })
    if (($commands -join ',') -cne 'capture,capture,replay,replay,validate,validate,validate,validate,validate,validate,compare,compare') {
        throw 'Smoke skipped or reordered its capture/replay/validation/comparison commands.'
    }
    $validators = @($state.Calls | Where-Object { $_[0] -eq 'validate' } | ForEach-Object { $_[1] })
    if (($validators -join ',') -cne 'registry,trace,linked,bundle,trace,linked') {
        throw 'Smoke omitted a native validator.'
    }
    Assert-SmokeRun
    if ($state.Scratch -ceq $firstScratch) {
        throw 'Separate smoke invocations reused mutable scratch or build intermediates.'
    }
    Assert-SmokeRun -Failure build -ExpectedError 'Failed to build'
    Assert-SmokeRun -Artifacts 0 -ExpectedError 'found 0'
    Assert-SmokeRun -Artifacts 2 -ExpectedError 'found 2'
    foreach ($command in @('capture', 'replay', 'validate', 'compare')) {
        Assert-SmokeRun -Failure $command -ExpectedError 'specgate failed'
        if ($state.Calls[-1][0] -cne $command) {
            throw 'Smoke continued executing commands after a CLI failure.'
        }
    }
    Assert-SmokeRun -Failure mismatch-exit -ExpectedError 'Expected changed result comparison to exit 1'
    Assert-SmokeRun -Failure mismatch-path -ExpectedError 'did not report a semantic mismatch path'
}
finally {
    $env:CARGO_BUILD_BUILD_DIR = $previousBuildDir
    Remove-Item -LiteralPath $fixture -Recurse -Force
}

Write-Output 'CTSC smoke wrapper regressions passed.'
