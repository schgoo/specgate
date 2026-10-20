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
    $wrapperRoot = Join-Path ([IO.Path]::GetTempPath()) "sg-wrapper-tests-$([guid]::NewGuid().ToString('N'))"
    $wrapperScripts = Join-Path $wrapperRoot 'scripts'
    New-Item -ItemType Directory -Force $wrapperScripts | Out-Null
    foreach ($name in @('ctsc-goldens.ps1', 'ctsc-goldens-support.ps1')) {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $wrapperScripts
    }
    $wrapper = Join-Path $wrapperScripts 'ctsc-goldens.ps1'
    $names = @('SPECGATE_CACHE_DIR', 'SPECGATE_CTSC_GOLDENS', 'SPECGATE_CTSC_GOLDENS_SCRATCH', 'CARGO_BUILD_BUILD_DIR')
    $previous = @{}
    foreach ($name in $names) {
        $previous[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    $previousCacheRoot = $env:SPECGATE_CTSC_GOLDENS_BUILD_CACHE
    $state = @{
        Scratch = $null; CachePath = $null; Lease = $null; Fail = $false; InvalidProof = $false; Builds = 0; Executions = 0
        CacheRoot = Join-Path $wrapperRoot 'rust/target/ctsc-goldens-build-cache/v2'
        Override = $null
    }

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
        $state.CachePath = $env:CARGO_BUILD_BUILD_DIR
        $state.Lease = $buildCache.Lease
        $cacheRoot = $state.CacheRoot
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
        if ($state.InvalidProof) {
            Write-Output 'running 0 tests'
            return
        }
        Write-Output 'running 1 test'
        Write-Output 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s'
    }

    function Assert-WrapperCleanup {
        param([bool]$Reusable = $true)

        foreach ($name in $names) {
            if ([Environment]::GetEnvironmentVariable($name) -cne "caller-$name") {
                throw "The golden wrapper did not restore $name."
            }
        }
        if ($env:SPECGATE_CTSC_GOLDENS_BUILD_CACHE -cne $state.Override) {
            throw 'The golden wrapper changed the caller build-cache root override.'
        }
        if (Test-Path -LiteralPath $state.Scratch) {
            throw 'The golden wrapper leaked invocation scratch.'
        }
        if ($state.Lease.CanRead) {
            throw 'The golden wrapper did not release its build-cache lease.'
        }
        $leaseFile = Join-Path (Split-Path -Parent $state.CachePath) 'lease'
        if ((([IO.File]::ReadAllBytes($leaseFile)).Length -eq 0) -ne $Reusable) {
            throw 'The golden wrapper marked an incompletely finished cache reusable, or failed to recycle a successful cache.'
        }
    }

    try {
        foreach ($name in $names) {
            [Environment]::SetEnvironmentVariable($name, "caller-$name")
        }
        $env:SPECGATE_CTSC_GOLDENS_BUILD_CACHE = $null
        $previousPath = $null
        foreach ($mode in @('Check', 'Update')) {
            & $wrapper -Mode $mode | Out-Null
            Assert-WrapperCleanup
            if ($null -ne $previousPath -and $state.CachePath -cne $previousPath) {
                throw 'Successful golden invocations did not reuse their cache slot.'
            }
            $previousPath = $state.CachePath
        }
        $state.Fail = $true
        Assert-Throws {
            & $wrapper -Mode Check | Out-Null
        } 'CTSC golden check failed'
        Assert-WrapperCleanup -Reusable $false
        $state.Fail = $false
        $state.InvalidProof = $true
        Assert-Throws {
            & $wrapper -Mode Update | Out-Null
        } 'did not prove exactly one'
        Assert-WrapperCleanup -Reusable $false
        $state.InvalidProof = $false
        $state.CacheRoot = Join-Path $wrapperRoot 'artifacts/ctsc-goldens-build-cache/v2'
        $previousPath = $null
        foreach ($override in @($state.CacheRoot, 'artifacts/ctsc-goldens-build-cache/v2')) {
            $state.Override = $override
            $env:SPECGATE_CTSC_GOLDENS_BUILD_CACHE = $override
            & $wrapper -Mode Check | Out-Null
            Assert-WrapperCleanup
            if ($null -ne $previousPath -and $state.CachePath -cne $previousPath) {
                throw 'Absolute and repository-relative cache roots did not select the same reusable slot.'
            }
            $previousPath = $state.CachePath
        }
        if ($state.Builds -ne 6 -or $state.Executions -ne 6) {
            throw 'The wrapper regressions did not exercise all six harness invocations.'
        }
    }
    finally {
        foreach ($name in $previous.Keys) {
            [Environment]::SetEnvironmentVariable($name, $previous[$name])
        }
        $env:SPECGATE_CTSC_GOLDENS_BUILD_CACHE = $previousCacheRoot
        Remove-Item -LiteralPath $wrapperRoot -Recurse -Force
    }
}

$fixture = Join-Path ([IO.Path]::GetTempPath()) "sg-cache-tests-$([guid]::NewGuid().ToString('N'))"
$first = $null
$second = $null
$reused = $null
$replacement = $null
$previousBuildDir = $env:CARGO_BUILD_BUILD_DIR
try {
    $cacheRoot = Join-Path $fixture 'cache'
    $first = Enter-GoldenBuildCache -Root $cacheRoot
    $second = Enter-GoldenBuildCache -Root $cacheRoot
    if ($first.Path -ceq $second.Path) {
        throw 'Concurrent invocations shared mutable build intermediates.'
    }
    Complete-GoldenBuildCache -Lease $first.Lease
    $first.Lease.Dispose()
    $reused = Enter-GoldenBuildCache -Root $cacheRoot
    if ($reused.Path -cne $first.Path) {
        throw 'A released cache slot was not reused.'
    }

    $wrapperScripts = Join-Path $fixture 'scripts'
    New-Item -ItemType Directory -Force $wrapperScripts | Out-Null
    foreach ($name in @('ctsc-goldens.ps1', 'ctsc-goldens-support.ps1')) {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $wrapperScripts
    }
    $cacheUser = Join-Path $fixture 'cache-user.ps1'
    [IO.File]::WriteAllText($cacheUser, @'
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $env:CARGO_BUILD_BUILD_DIR | Out-Null
$file = [IO.File]::Open((Join-Path $env:CARGO_BUILD_BUILD_DIR 'active-child'), 'OpenOrCreate', 'ReadWrite', 'ReadWrite')
try {
    [Console]::WriteLine((@{ Pid = $PID; Path = $env:CARGO_BUILD_BUILD_DIR } | ConvertTo-Json -Compress))
    for ($iteration = 0; $iteration -lt 600; $iteration++) {
        $file.WriteByte(1)
        $file.Flush()
        Start-Sleep -Milliseconds 100
    }
}
finally {
    $file.Dispose()
}
'@)
    $worker = Join-Path $fixture 'wrapper-worker.ps1'
    [IO.File]::WriteAllText($worker, @'
param($Wrapper, $CacheUser)
$ErrorActionPreference = 'Stop'
$env:SPECGATE_CTSC_GOLDENS_BUILD_CACHE = $null
function cargo {
    $global:LASTEXITCODE = 0
    @{
        reason = 'compiler-artifact'
        profile = @{ test = $true }
        target = @{ name = 'specgate_cli'; kind = @('lib') }
        executable = 'Invoke-ChildGoldenHarness'
    } | ConvertTo-Json -Compress
}
function Invoke-ChildGoldenHarness {
    $global:LASTEXITCODE = 0
    if ($args -contains '--list') {
        'goldens::tests::golden_matrix: test'
        return
    }
    & (Get-Process -Id $PID).Path -NoProfile -NonInteractive -File $CacheUser
}
& $Wrapper -Mode Check
'@)
    $start = [Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path)
    $start.UseShellExecute = $false
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in @('-NoProfile', '-NonInteractive', '-File', $worker, (Join-Path $wrapperScripts 'ctsc-goldens.ps1'), $cacheUser)) {
        $start.ArgumentList.Add($argument)
    }
    $child = [Diagnostics.Process]::Start($start)
    $cacheChild = $null
    try {
        $line = $child.StandardOutput.ReadLineAsync()
        if (-not $line.Wait(15000) -or [string]::IsNullOrEmpty($line.Result)) {
            throw 'The wrapper did not start its native cache-using child.'
        }
        $active = $line.Result | ConvertFrom-Json
        $cacheChild = [Diagnostics.Process]::GetProcessById($active.Pid)
        $wrapperCacheRoot = Join-Path $fixture 'rust/target/ctsc-goldens-build-cache/v2'
        $replacement = Enter-GoldenBuildCache -Root $wrapperCacheRoot
        if ($replacement.Path -ceq $active.Path) {
            throw 'Separate processes shared a live cache slot.'
        }
        Complete-GoldenBuildCache -Lease $replacement.Lease
        $replacement.Lease.Dispose()

        Stop-Process -Id $child.Id -Force
        $child.WaitForExit()
        if ($cacheChild.HasExited) {
            throw 'The regression did not leave a surviving native child.'
        }
        $marker = Join-Path $active.Path 'active-child'
        $before = (Get-Item -LiteralPath $marker).Length
        $wait = [Diagnostics.Stopwatch]::StartNew()
        do {
            Start-Sleep -Milliseconds 50
            $after = (Get-Item -LiteralPath $marker).Length
        } while ($after -le $before -and $wait.Elapsed.TotalSeconds -lt 5)
        if ($after -le $before) {
            throw 'The surviving native child did not keep writing to its cache slot.'
        }
        $replacement = Enter-GoldenBuildCache -Root $wrapperCacheRoot
        if ($replacement.Path -ceq $active.Path) {
            throw 'A terminated wrapper made its surviving child cache slot reusable.'
        }
        if ($cacheChild.HasExited) {
            throw 'The native child exited before cache isolation was checked.'
        }
    }
    finally {
        if (-not $child.HasExited) {
            Stop-Process -Id $child.Id -Force
        }
        $child.WaitForExit()
        $child.Dispose()
        if ($null -ne $cacheChild) {
            if (-not $cacheChild.HasExited) {
                Stop-Process -Id $cacheChild.Id -Force
            }
            $cacheChild.WaitForExit()
            $cacheChild.Dispose()
        }
    }
    $replacement.Lease.Dispose()

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
    foreach ($entry in @($first, $second, $reused, $replacement)) {
        if ($null -ne $entry) {
            $entry.Lease.Dispose()
        }
    }
    if (Test-Path -LiteralPath $fixture) {
        Remove-Item -LiteralPath $fixture -Recurse -Force
    }
}

Write-Host 'CTSC golden wrapper regressions passed.'
