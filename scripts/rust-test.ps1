param(
    [ValidateSet('all', 'replay', 'other')]
    [string]$Shard = 'all',
    [ValidateSet('scratch', 'persistent')]
    [string]$BuildDir = 'scratch'
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
# Scratch builds every child from empty and shares the workspace target. Each
# persistent shard owns its target and child build directories, so concurrent
# shards never relink executables that another lane runs, and later runs reuse
# warm child builds.
$scratch = if ($BuildDir -eq 'persistent') {
    Join-Path $root "rust/target/rust-test/$Shard"
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
        # Line tables keep panic locations but drop most debug info. Children
        # skip incremental state: generated code and fixtures are not edited.
        # A persistent shard owns its target, so its profile cannot invalidate
        # other builds; scratch shares rust/target and keeps the caller profile.
        $cargoArguments = @('test', '--workspace')
        if ($BuildDir -eq 'persistent') {
            $cargoArguments += @(
                '--target-dir', (Join-Path $scratch 'target'),
                '--config', "profile.dev.debug='line-tables-only'"
            )
        }
        $cargoArguments += @(
            '--config', "env.CARGO_BUILD_BUILD_DIR.value=$childBuildDir",
            '--config', 'env.CARGO_BUILD_BUILD_DIR.force=true',
            '--config', "env.CARGO_PROFILE_DEV_DEBUG='line-tables-only'",
            '--config', "env.CARGO_INCREMENTAL='0'",
            '--', '--test-threads=1', '--skip', 'goldens::tests::cases'
        )
        switch ($Shard) {
            'replay' { $cargoArguments += 'replay::tests' }
            'other' { $cargoArguments += '--skip', 'replay::tests' }
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
