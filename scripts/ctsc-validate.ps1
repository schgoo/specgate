$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $root 'rust/Cargo.toml'

& cargo test --manifest-path $manifest -p specgate-ctsc --test native_validation
if ($LASTEXITCODE -ne 0) {
    throw 'Native CTSC corpus validation failed.'
}
