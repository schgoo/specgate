$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'ctsc-goldens-support.ps1')

function Assert-Throws {
    param(
        [Parameter(Mandatory)]
        [scriptblock]$Action,
        [Parameter(Mandatory)]
        [string]$MessageFragment
    )

    try {
        & $Action
    }
    catch {
        if ($_.Exception.Message -notlike "*$MessageFragment*") {
            throw "Expected failure containing '$MessageFragment', got: $($_.Exception.Message)"
        }
        return
    }
    throw "Expected action to fail with '$MessageFragment'."
}

$testName = 'goldens::tests::golden_matrix'
$harnessMessage = @{
    reason = 'compiler-artifact'
    profile = @{ test = $true }
    target = @{ name = 'specgate_cli'; kind = @('lib') }
    executable = 'specgate-cli-tests'
} | ConvertTo-Json -Compress

if ((Get-GoldenHarnessExecutable -CargoMessages @($harnessMessage)) -cne 'specgate-cli-tests') {
    throw 'The expected golden harness executable was not selected.'
}
Assert-Throws {
    Get-GoldenHarnessExecutable -CargoMessages @()
} 'found 0'
Assert-Throws {
    Get-GoldenHarnessExecutable -CargoMessages @(
        $harnessMessage,
        ($harnessMessage | ConvertFrom-Json | ForEach-Object {
                $_.executable = 'duplicate-specgate-cli-tests'
                $_
            } | ConvertTo-Json -Compress)
    )
} 'found 2'

Assert-SingleGoldenTestSelection -Lines @("$testName`: test") -TestName $testName
Assert-Throws {
    Assert-SingleGoldenTestSelection -Lines @('goldens::renamed_test: test') -TestName $testName
} 'found 1'
Assert-Throws {
    Assert-SingleGoldenTestSelection -Lines @() -TestName $testName
} 'found 0'

Assert-SingleGoldenTestExecution -Lines @(
    'running 1 test',
    'test goldens::tests::golden_matrix ... ok',
    'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 99 filtered out; finished in 0.01s'
)
Assert-Throws {
    Assert-SingleGoldenTestExecution -Lines @(
        'running 0 tests',
        'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 100 filtered out; finished in 0.00s'
    )
} 'did not prove exactly one'

& {
    $names = @('SPECGATE_CACHE_DIR', 'SPECGATE_CTSC_GOLDENS', 'SPECGATE_CTSC_GOLDENS_SCRATCH', 'CARGO_BUILD_BUILD_DIR')
    $previous = @{}
    foreach ($name in $names) {
        $previous[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    $state = @{ Scratch = $null; Lease = $null; Fail = $false; Builds = 0; Executions = 0 }

    function cargo {
        if ($env:CARGO_BUILD_BUILD_DIR -cne 'caller-CARGO_BUILD_BUILD_DIR') {
            throw 'The golden wrapper changed the harness build directory.'
        }
        $state.Builds++
        $global:LASTEXITCODE = 0
        @{
            reason = 'compiler-artifact'
            profile = @{ test = $true }
            target = @{ name = 'specgate_cli'; kind = @('lib') }
            executable = 'Invoke-TestGoldenHarness'
        } | ConvertTo-Json -Compress
    }

    function Invoke-TestGoldenHarness {
        $global:LASTEXITCODE = 0
        if ($args -contains '--list') {
            Write-Output 'goldens::tests::golden_matrix: test'
            return
        }
        $state.Executions++
        $state.Scratch = $env:SPECGATE_CTSC_GOLDENS_SCRATCH
        $state.Lease = $buildCache.Lease
        $cacheRoot = Join-Path (Split-Path -Parent $PSScriptRoot) 'rust/target/ctsc-goldens-build-cache'
        if (-not $env:CARGO_BUILD_BUILD_DIR.StartsWith("$cacheRoot$([IO.Path]::DirectorySeparatorChar)", [StringComparison]::Ordinal) -or
            -not $state.Lease.CanRead -or
            $env:SPECGATE_CACHE_DIR -cne (Join-Path $state.Scratch 'cache')) {
            throw 'The golden harness did not receive exclusively leased build intermediates and isolated scratch.'
        }
        if ($state.Fail) {
            Write-Output 'injected golden failure'
            $global:LASTEXITCODE = 23
            return
        }
        Write-Output 'running 1 test'
        Write-Output 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s'
    }

    function Assert-WrapperCleanup {
        foreach ($name in $names) {
            if ([Environment]::GetEnvironmentVariable($name) -cne "caller-$name") {
                throw "The golden wrapper did not restore $name."
            }
        }
        if (Test-Path -LiteralPath $state.Scratch) {
            throw 'The golden wrapper leaked invocation scratch.'
        }
        if ($state.Lease.CanRead) {
            throw 'The golden wrapper did not release its build-cache lease.'
        }
    }

    try {
        foreach ($name in $names) {
            [Environment]::SetEnvironmentVariable($name, "caller-$name")
        }
        foreach ($mode in @('Check', 'Update')) {
            & (Join-Path $PSScriptRoot 'ctsc-goldens.ps1') -Mode $mode | Out-Null
            Assert-WrapperCleanup
        }
        $state.Fail = $true
        Assert-Throws {
            & (Join-Path $PSScriptRoot 'ctsc-goldens.ps1') -Mode Check | Out-Null
        } 'CTSC golden check failed'
        Assert-WrapperCleanup
        if ($state.Builds -ne 3 -or $state.Executions -ne 3) {
            throw 'The wrapper regressions did not exercise all three harness invocations.'
        }
    }
    finally {
        foreach ($name in $previous.Keys) {
            [Environment]::SetEnvironmentVariable($name, $previous[$name])
        }
    }
}

$fixture = Join-Path ([IO.Path]::GetTempPath()) "sg-cache-tests-$([guid]::NewGuid().ToString('N'))"
$first = $null
$second = $null
$reused = $null
$recovered = $null
$previousBuildDir = $env:CARGO_BUILD_BUILD_DIR
try {
    $cacheRoot = Join-Path $fixture 'cache'
    $first = Enter-GoldenBuildCache -Root $cacheRoot
    $second = Enter-GoldenBuildCache -Root $cacheRoot
    if ($first.Path -ceq $second.Path) {
        throw 'Concurrent invocations shared mutable build intermediates.'
    }
    $first.Lease.Dispose()
    $reused = Enter-GoldenBuildCache -Root $cacheRoot
    if ($reused.Path -cne $first.Path) {
        throw 'A released cache slot was not reused.'
    }

    $worker = Join-Path $fixture 'lease-worker.ps1'
    [IO.File]::WriteAllText($worker, @'
param($Helper, $Root)
$ErrorActionPreference = 'Stop'
. $Helper
$cache = Enter-GoldenBuildCache -Root $Root
[Console]::WriteLine($cache.Path)
Start-Sleep -Seconds 60
$cache.Lease.Dispose()
'@)
    $start = [Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path)
    $start.UseShellExecute = $false
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in @('-NoProfile', '-NonInteractive', '-File', $worker, (Join-Path $PSScriptRoot 'ctsc-goldens-support.ps1'), $cacheRoot)) {
        $start.ArgumentList.Add($argument)
    }
    $child = [Diagnostics.Process]::Start($start)
    try {
        $line = $child.StandardOutput.ReadLineAsync()
        if (-not $line.Wait(15000) -or [string]::IsNullOrEmpty($line.Result)) {
            throw 'The cache-lease worker did not acquire a slot.'
        }
        $childPath = $line.Result
        if ($childPath -ceq $reused.Path -or $childPath -ceq $second.Path) {
            throw 'Separate processes shared a leased build cache.'
        }
    }
    finally {
        if (-not $child.HasExited) {
            Stop-Process -Id $child.Id -Force
        }
        $child.WaitForExit()
        $child.Dispose()
    }
    $recovered = Enter-GoldenBuildCache -Root $cacheRoot
    if ($recovered.Path -cne $childPath) {
        throw 'A terminated process left a stale build-cache lease.'
    }
    $recovered.Lease.Dispose()

    $invalidRoot = Join-Path $fixture 'invalid-cache'
    New-Item -ItemType Directory -Force (Join-Path $invalidRoot '0/lease') | Out-Null
    $rejected = $false
    try {
        $unexpected = Enter-GoldenBuildCache -Root $invalidRoot
        $unexpected.Lease.Dispose()
    }
    catch [UnauthorizedAccessException] {
        $rejected = $true
    }
    if (-not $rejected) {
        throw 'A non-contention cache error was silently bypassed.'
    }

    $dependency = Join-Path $fixture 'dependency'
    New-Item -ItemType Directory -Force (Join-Path $dependency 'src') | Out-Null
    [IO.File]::WriteAllText((Join-Path $dependency 'Cargo.toml'), "[package]`nname = `"golden-cache-dependency`"`nversion = `"0.0.0`"`nedition = `"2024`"`n")
    $dependencySource = Join-Path $dependency 'src/lib.rs'
    [IO.File]::WriteAllText($dependencySource, 'pub fn value() -> u32 { 1 }')
    $env:CARGO_BUILD_BUILD_DIR = $reused.Path
    $targets = @()
    foreach ($sample in @('first', 'second', 'changed')) {
        $runner = Join-Path $fixture $sample
        New-Item -ItemType Directory -Force (Join-Path $runner 'src') | Out-Null
        $manifest = Join-Path $runner 'Cargo.toml'
        [IO.File]::WriteAllText($manifest, "[package]`nname = `"golden-cache-runner`"`nversion = `"0.0.0`"`nedition = `"2024`"`n[dependencies]`ngolden-cache-dependency = { path = `"../dependency`" }`n")
        $offset = if ($sample -eq 'first') { 0 } else { 10 }
        [IO.File]::WriteAllText((Join-Path $runner 'src/main.rs'), "fn main() { println!(`"{}`", golden_cache_dependency::value() + $offset); }")
        if ($sample -eq 'changed') {
            [IO.File]::WriteAllText($dependencySource, 'pub fn value() -> u32 { 200 }')
        }
        $target = Join-Path $runner 'target'
        $targets += $target
        $messages = @(& cargo build --manifest-path $manifest --target-dir $target --quiet --message-format=json)
        if ($LASTEXITCODE -ne 0) {
            throw "Cache regression '$sample' failed to compile."
        }
        $artifacts = @($messages | ConvertFrom-Json | Where-Object { $_.reason -eq 'compiler-artifact' })
        $libraries = @($artifacts | Where-Object { $_.target.name -eq 'golden_cache_dependency' })
        $binaries = @($artifacts | Where-Object { $_.target.name -eq 'golden-cache-runner' })
        if ($libraries.Count -ne 1 -or $binaries.Count -ne 1) {
            throw "Cache regression '$sample' did not produce exactly one dependency and runner."
        }
        if ($libraries[0].fresh -ne ($sample -eq 'second')) {
            throw "Cache regression '$sample' did not reuse or invalidate the dependency as expected."
        }
        $actual = @(& $binaries[0].executable)
        $expected = switch ($sample) { first { '1' }; second { '11' }; changed { '210' } }
        if ($LASTEXITCODE -ne 0 -or $actual.Count -ne 1 -or $actual[0] -cne $expected) {
            throw "Cache regression '$sample' executed stale or incorrect code."
        }
        if (-not $binaries[0].executable.StartsWith("$target$([IO.Path]::DirectorySeparatorChar)", [StringComparison]::Ordinal)) {
            throw "Cache regression '$sample' did not isolate its final executable."
        }
    }
    foreach ($target in $targets) {
        if (-not (Test-Path -LiteralPath $target)) {
            throw 'Building another runner removed an earlier runner output.'
        }
    }
}
finally {
    $env:CARGO_BUILD_BUILD_DIR = $previousBuildDir
    foreach ($entry in @($first, $second, $reused, $recovered)) {
        if ($null -ne $entry) {
            $entry.Lease.Dispose()
        }
    }
    if (Test-Path -LiteralPath $fixture) {
        Remove-Item -LiteralPath $fixture -Recurse -Force
    }
}

Write-Host 'CTSC golden wrapper regressions passed.'
