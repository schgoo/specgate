$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$scratch = Join-Path $root "rust/target/rust-test-$([guid]::NewGuid().ToString('N'))"
try {
    New-Item -ItemType Directory -Path $scratch | Out-Null
    $childBuildDir = ConvertTo-Json -InputObject (Join-Path $scratch 'build') -Compress
    Push-Location (Join-Path $root 'rust')
    try {
        # Cargo's env table affects children, not its own workspace compilation.
        # Finish each test before another can rebuild its generated executables.
        $cargoArguments = @(
            'test', '--workspace',
            '--config', "env.CARGO_BUILD_BUILD_DIR.value=$childBuildDir",
            '--config', 'env.CARGO_BUILD_BUILD_DIR.force=true',
            '--', '--test-threads=1'
        )
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
    if (Test-Path -LiteralPath $scratch) {
        Remove-Item -LiteralPath $scratch -Recurse -Force
    }
}
