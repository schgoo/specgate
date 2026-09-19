$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$rust = Join-Path $root 'rust'
$scratch = Join-Path $rust "target/package-smoke-$PID"
$cargoHome = Join-Path $scratch 'cargo-home'
$install = Join-Path $scratch 'install'
$candidate = Join-Path $scratch 'candidate'
$capture = Join-Path $scratch 'capture'
$registry = Join-Path $scratch 'registry.ctsc.json'
$replay = Join-Path $scratch 'candidate.otlp.json'

function Cargo-Path([string]$Path) {
    return $Path.Replace('\', '/')
}

function Invoke-Checked {
    param([string]$Program, [string[]]$Arguments)

    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Program failed: $($Arguments -join ' ')"
    }
}

try {
    New-Item -ItemType Directory -Force $cargoHome, (Join-Path $candidate 'src') | Out-Null

    Push-Location $rust
    try {
        Invoke-Checked cargo @('package', '--workspace', '--allow-dirty', '--no-verify')
    }
    finally {
        Pop-Location
    }

    $archives = Join-Path $rust 'target/package'
    $packageRoot = Join-Path $scratch 'packages'
    New-Item -ItemType Directory -Force $packageRoot | Out-Null
    foreach ($name in @(
        'specgate',
        'specgate-annotations-macros',
        'specgate-runtime',
        'specgate-discovery',
        'specgate-ctsc',
        'specgate-cli'
    )) {
        Invoke-Checked tar @(
            '-xzf',
            (Join-Path $archives "$name-0.6.0.crate"),
            '-C',
            $packageRoot
        )
        Add-Content -Path (Join-Path $packageRoot "$name-0.6.0/Cargo.toml") -Value "`n[workspace]"
    }
    $config = @"
[patch.crates-io]
specgate = { path = "$(Cargo-Path (Join-Path $packageRoot 'specgate-0.6.0'))" }
specgate-annotations-macros = { path = "$(Cargo-Path (Join-Path $packageRoot 'specgate-annotations-macros-0.6.0'))" }
specgate-runtime = { path = "$(Cargo-Path (Join-Path $packageRoot 'specgate-runtime-0.6.0'))" }
specgate-discovery = { path = "$(Cargo-Path (Join-Path $packageRoot 'specgate-discovery-0.6.0'))" }
specgate-ctsc = { path = "$(Cargo-Path (Join-Path $packageRoot 'specgate-ctsc-0.6.0'))" }
"@
    Set-Content -Path (Join-Path $cargoHome 'config.toml') -Value $config -NoNewline

    $env:CARGO_HOME = $cargoHome
    $env:SPECGATE_CACHE_DIR = Join-Path $scratch 'runtime-cache'
    Remove-Item Env:SPECGATE_RUNTIME_PATH -ErrorAction SilentlyContinue
    Remove-Item Env:SPECGATE_RUNTIME_VERSION -ErrorAction SilentlyContinue

    Invoke-Checked cargo @(
        'install',
        '--path', (Join-Path $packageRoot 'specgate-cli-0.6.0'),
        '--root', $install,
        '--debug'
    )

    Set-Content -Path (Join-Path $candidate 'Cargo.toml') -NoNewline -Value @'
[package]
name = "packaged-candidate"
version = "0.1.0"
edition = "2024"

[dependencies]
specgate = "0.6.0"

[workspace]
'@
    Set-Content -Path (Join-Path $candidate 'src/lib.rs') -NoNewline -Value @'
use specgate::{spec_component, spec_operation};

spec_component!("fixture.packaged");

#[spec_operation("add")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[test]
fn add_two_and_three() {
    assert_eq!(add(2, 3), 5);
}
'@
    $binding = Join-Path $scratch 'binding.yaml'
    Set-Content -Path $binding -NoNewline -Value @"
language: rust
targets:
  default:
    package_root: $(Cargo-Path $candidate)
"@

    $binary = Join-Path $install 'bin/specgate'
    if ($IsWindows) {
        $binary += '.exe'
    }
    Invoke-Checked $binary @(
        'discover', $binding,
        '--component', 'fixture.packaged',
        '--registry-id', 'urn:ctsc:registry:fixture.packaged',
        '--registry-version', '0.1.0',
        '--out', $registry
    )
    Invoke-Checked $binary @('capture', $binding, '--component', 'fixture.packaged', '--out', $capture)
    Invoke-Checked $binary @('replay', $capture, $binding, '--out', $replay)

    Invoke-Checked python @((Join-Path $root 'docs/ctsc/validate.py'), 'registry', $registry)
    Invoke-Checked python @((Join-Path $root 'docs/ctsc/validate.py'), 'linked', (Join-Path $capture 'reference.otlp.json'), (Join-Path $capture 'registry.ctsc.json'))
    Invoke-Checked python @((Join-Path $root 'docs/ctsc/validate.py'), 'linked', $replay, (Join-Path $capture 'registry.ctsc.json'))

    $nuget = Join-Path $scratch 'nuget-a'
    $nugetRepeat = Join-Path $scratch 'nuget-b'
    $env:SOURCE_DATE_EPOCH = '315532800'
    Invoke-Checked dotnet @(
        'pack',
        (Join-Path $root 'csharp/SpecGate.Annotations/SpecGate.Annotations.csproj'),
        '-c', 'Release',
        '-p:ContinuousIntegrationBuild=true',
        '-o', $nuget
    )
    Invoke-Checked dotnet @(
        'pack',
        (Join-Path $root 'csharp/SpecGate.Annotations/SpecGate.Annotations.csproj'),
        '-c', 'Release',
        '-p:ContinuousIntegrationBuild=true',
        '--no-restore',
        '-o', $nugetRepeat
    )
    $package = Get-ChildItem $nuget -Filter 'SpecGate.Annotations.0.6.0.nupkg' | Select-Object -First 1
    $packageRepeat = Get-ChildItem $nugetRepeat -Filter 'SpecGate.Annotations.0.6.0.nupkg' | Select-Object -First 1
    if ($null -eq $package -or $null -eq $packageRepeat) {
        throw 'C# annotations package was not produced at version 0.6.0.'
    }
    if ((Get-FileHash $package.FullName -Algorithm SHA256).Hash -ne
        (Get-FileHash $packageRepeat.FullName -Algorithm SHA256).Hash) {
        throw 'C# annotations package is not byte-deterministic.'
    }

    Write-Output 'Packaged-context discover/capture/replay smoke passed.'
}
finally {
    Remove-Item Env:CARGO_HOME -ErrorAction SilentlyContinue
    Remove-Item Env:SPECGATE_CACHE_DIR -ErrorAction SilentlyContinue
    Remove-Item Env:SPECGATE_RUNTIME_PATH -ErrorAction SilentlyContinue
    Remove-Item Env:SPECGATE_RUNTIME_VERSION -ErrorAction SilentlyContinue
    Remove-Item Env:SOURCE_DATE_EPOCH -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
}
