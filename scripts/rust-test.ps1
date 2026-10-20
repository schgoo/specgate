$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$scratch = Join-Path $root "rust/target/rust-test-$([guid]::NewGuid().ToString('N'))"
$previousBuildDir = $env:CARGO_BUILD_BUILD_DIR

try {
    New-Item -ItemType Directory -Path $scratch | Out-Null
    $env:CARGO_BUILD_BUILD_DIR = Join-Path $scratch 'build'
    Push-Location (Join-Path $root 'rust')
    try {
        # Finish each test before another can rebuild its generated executables.
        $cargoArguments = @('test', '--workspace', '--', '--test-threads=1')
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
    $env:CARGO_BUILD_BUILD_DIR = $previousBuildDir
    if (Test-Path -LiteralPath $scratch) {
        Remove-Item -LiteralPath $scratch -Recurse -Force
    }
}
