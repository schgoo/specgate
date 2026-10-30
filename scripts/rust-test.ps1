param(
    [ValidateSet('all', 'replay', 'other', 'isolated', 'shared-fixture')]
    [string]$Shard = 'all',
    [ValidateSet('scratch', 'persistent')]
    [string]$BuildDir = 'scratch'
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
# Scratch builds every child from empty and shares the workspace target. Each
# persistent shard owns its target and child build directories, so concurrent
# shards never relink executables that another lane runs, and later runs reuse
# warm child builds. The local isolated and shared-fixture shards split other,
# so they reuse its directories.
$scratch = if ($BuildDir -eq 'persistent') {
    $owner = if ($Shard -in 'isolated', 'shared-fixture') { 'other' } else { $Shard }
    Join-Path $root "rust/target/rust-test/$owner"
}
else {
    Join-Path $root "rust/target/rust-test-$([guid]::NewGuid().ToString('N'))"
}
try {
    New-Item -ItemType Directory -Force -Path $scratch | Out-Null
    $childBuildDir = ConvertTo-Json -InputObject (Join-Path $scratch 'build') -Compress
    Push-Location (Join-Path $root 'rust')
    try {
        # Cargo's env table affects children, not its own workspace compilation.
        # Finish each test before another can rebuild its generated executables.
        # just ctsc-goldens-harness-tests owns the golden harness cases. CI shards
        # split on one libtest name filter, so replay and other are complements.
        # Tests in a shared_fixture module lease a golden-matrix fixture package
        # that ctsc-smoke and ctsc-goldens also lease. Local parallel checks run
        # them after those lanes; isolated and shared-fixture split other.
        $cargoArguments = @('test', '--workspace')
        if ($BuildDir -eq 'persistent') {
            $cargoArguments += '--target-dir', (Join-Path $scratch 'target')
        }
        $cargoArguments += @(
            '--config', "env.CARGO_BUILD_BUILD_DIR.value=$childBuildDir",
            '--config', 'env.CARGO_BUILD_BUILD_DIR.force=true',
            '--', '--test-threads=1', '--skip', 'goldens::tests::cases'
        )
        switch ($Shard) {
            'replay' { $cargoArguments += 'replay::tests' }
            'other' { $cargoArguments += '--skip', 'replay::tests' }
            'isolated' { $cargoArguments += '--skip', 'replay::tests', '--skip', 'shared_fixture::' }
            'shared-fixture' { $cargoArguments += '--skip', 'replay::tests', 'shared_fixture::' }
        }
        & cargo @cargoArguments
        if ($LASTEXITCODE -ne 0) {
            throw 'Rust workspace tests failed.'
        }
    }
    finally {
        Pop-Location
    }
}
finally {
    if ($BuildDir -eq 'scratch' -and (Test-Path -LiteralPath $scratch)) {
        Remove-Item -LiteralPath $scratch -Recurse -Force
    }
}
