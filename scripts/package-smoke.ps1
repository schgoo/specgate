$ErrorActionPreference = 'Stop'

$previousRustupToolchain = $env:RUSTUP_TOOLCHAIN
$env:RUSTUP_TOOLCHAIN = '1.96.0'

$root = Split-Path -Parent $PSScriptRoot
$rust = Join-Path $root 'rust'
$scratch = Join-Path $rust "target/package-smoke-$PID"
$cargoHome = Join-Path $scratch 'cargo-home'
$install = Join-Path $scratch 'install'
$candidate = Join-Path $scratch 'candidate'
$capture = Join-Path $scratch 'capture'
$registry = Join-Path $scratch 'registry.ctsc.json'
$rustup = (Get-Command rustup -ErrorAction Stop).Source
$cargo = & $rustup which cargo --toolchain $env:RUSTUP_TOOLCHAIN
$rustc = & $rustup which rustc --toolchain $env:RUSTUP_TOOLCHAIN
$rustdoc = & $rustup which rustdoc --toolchain $env:RUSTUP_TOOLCHAIN
if ($LASTEXITCODE -ne 0 -or !$cargo -or !$rustc -or !$rustdoc) {
    throw "Rust toolchain '$env:RUSTUP_TOOLCHAIN' is not installed with cargo, rustc, and rustdoc."
}
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

# Packs one project from a clean Release tree, so the result reflects a full
# compile rather than a repackaged incremental output. Both packs of a run build
# through the same paths, leaving the commit as the only shared input.
function Invoke-DeterministicPack {
    param([string]$ProjectDirectory, [string]$OutputDirectory)

    $name = Split-Path -Leaf $ProjectDirectory
    foreach ($stale in @('bin/Release', 'obj/Release')) {
        Remove-Item -Recurse -Force (Join-Path $ProjectDirectory $stale) -ErrorAction SilentlyContinue
    }

    Invoke-Checked dotnet @(
        'pack',
        (Join-Path $ProjectDirectory "$name.csproj"),
        '-c', 'Release',
        '-p:ContinuousIntegrationBuild=true',
        '-o', $OutputDirectory
    ) | Out-Host

    $package = Get-ChildItem $OutputDirectory -Filter "$name.0.6.0.nupkg" | Select-Object -First 1
    if ($null -eq $package) {
        throw "$name package was not produced at version 0.6.0."
    }

    return $package.FullName
}

# Reports which parts of two packages differ. A package is an OPC zip, so naming
# the differing entries distinguishes a nondeterministic compile from a
# nondeterministic manifest, which a hash comparison alone cannot do.
function Write-PackageDifference {
    param([string]$Package, [string]$PackageRepeat)

    Add-Type -AssemblyName System.IO.Compression.FileSystem

    function Read-Entries([string]$Path) {
        $entries = [ordered]@{}
        $archive = [IO.Compression.ZipFile]::OpenRead($Path)
        try {
            foreach ($entry in $archive.Entries) {
                $entries[$entry.FullName] = [pscustomobject]@{
                    Length = $entry.Length
                    Crc = '{0:x8}' -f $entry.Crc32
                    LastWriteTime = $entry.LastWriteTime.ToString('o')
                }
            }
        }
        finally {
            $archive.Dispose()
        }

        return $entries
    }

    $left = Read-Entries $Package
    $right = Read-Entries $PackageRepeat

    Write-Output "Package A: $Package"
    Write-Output "Package B: $PackageRepeat"
    if (@($left.Keys) -join "`n" -ne (@($right.Keys) -join "`n")) {
        Write-Output "Entry order or membership differs:"
        Write-Output "  A: $(@($left.Keys) -join ', ')"
        Write-Output "  B: $(@($right.Keys) -join ', ')"
    }

    foreach ($name in $left.Keys) {
        if (-not $right.Contains($name)) {
            continue
        }

        $a = $left[$name]
        $b = $right[$name]
        if ($a.Length -ne $b.Length -or $a.Crc -ne $b.Crc -or $a.LastWriteTime -ne $b.LastWriteTime) {
            Write-Output "Entry differs: $name"
            Write-Output "  A: length=$($a.Length) crc=$($a.Crc) modified=$($a.LastWriteTime)"
            Write-Output "  B: length=$($b.Length) crc=$($b.Crc) modified=$($b.LastWriteTime)"
        }
    }
}

try {
    New-Item -ItemType Directory -Force $cargoHome, (Join-Path $candidate 'src') | Out-Null

    Push-Location $rust
    try {
        $packageArguments = @('package', '--workspace', '--allow-dirty', '--no-verify')
        foreach ($name in @(
            'specgate',
            'specgate-annotations-macros',
            'specgate-annotations-macros-impl',
            'specgate-runtime',
            'specgate-discovery',
            'specgate-ctsc',
            'specgate-cli'
        )) {
            $packageArguments += '--config'
            $packageArguments += "patch.crates-io.$name.path=`"crates/$name`""
        }
        Invoke-Checked $cargo $packageArguments
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
        'specgate-annotations-macros-impl',
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
specgate-annotations-macros-impl = { path = "$(Cargo-Path (Join-Path $packageRoot 'specgate-annotations-macros-impl-0.6.0'))" }
specgate-runtime = { path = "$(Cargo-Path (Join-Path $packageRoot 'specgate-runtime-0.6.0'))" }
specgate-discovery = { path = "$(Cargo-Path (Join-Path $packageRoot 'specgate-discovery-0.6.0'))" }
specgate-ctsc = { path = "$(Cargo-Path (Join-Path $packageRoot 'specgate-ctsc-0.6.0'))" }
"@
    Set-Content -Path (Join-Path $cargoHome 'config.toml') -Value $config -NoNewline

    $env:CARGO_HOME = $cargoHome
    $env:RUSTC = $rustc
    $env:RUSTDOC = $rustdoc
    $toolchainBin = Split-Path -Parent $cargo
    $env:PATH = "$toolchainBin$([IO.Path]::PathSeparator)$env:PATH"
    $env:SPECGATE_CACHE_DIR = Join-Path $scratch 'runtime-cache'
    Remove-Item Env:SPECGATE_RUNTIME_PATH -ErrorAction SilentlyContinue
    Remove-Item Env:SPECGATE_RUNTIME_VERSION -ErrorAction SilentlyContinue

    Invoke-Checked $cargo @(
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

    Invoke-Checked $binary @('validate', 'registry', $registry)
    Invoke-Checked $binary @('validate', 'linked', (Join-Path $capture 'reference.otlp.json'), (Join-Path $capture 'registry.ctsc.json'))
    Invoke-Checked $binary @('validate', 'linked', $replay, (Join-Path $capture 'registry.ctsc.json'))
    Invoke-Checked $binary @('validate', 'bundle', $capture)
    Invoke-Checked $binary @('compare', (Join-Path $capture 'reference.otlp.json'), $replay, '--registry', (Join-Path $capture 'registry.ctsc.json'))

    $nuget = Join-Path $scratch 'nuget-a'
    $nugetRepeat = Join-Path $scratch 'nuget-b'
    $env:SOURCE_DATE_EPOCH = '315532800'
    $annotations = Join-Path $root 'csharp/SpecGate.Annotations'
    $package = Invoke-DeterministicPack $annotations $nuget
    $packageRepeat = Invoke-DeterministicPack $annotations $nugetRepeat
    if ((Get-FileHash $package -Algorithm SHA256).Hash -ne
        (Get-FileHash $packageRepeat -Algorithm SHA256).Hash) {
        Write-PackageDifference $package $packageRepeat
        throw 'C# annotations package is not byte-deterministic.'
    }

    Write-Output 'Packaged-context discover/capture/replay/validate/compare smoke passed.'
}
finally {
    Remove-Item Env:CARGO_HOME -ErrorAction SilentlyContinue
    Remove-Item Env:SPECGATE_CACHE_DIR -ErrorAction SilentlyContinue
    Remove-Item Env:SPECGATE_RUNTIME_PATH -ErrorAction SilentlyContinue
    Remove-Item Env:SPECGATE_RUNTIME_VERSION -ErrorAction SilentlyContinue
    Remove-Item Env:SOURCE_DATE_EPOCH -ErrorAction SilentlyContinue
    Remove-Item Env:RUSTC -ErrorAction SilentlyContinue
    Remove-Item Env:RUSTDOC -ErrorAction SilentlyContinue
    if ($null -eq $previousRustupToolchain) {
        Remove-Item Env:RUSTUP_TOOLCHAIN -ErrorAction SilentlyContinue
    }
    else {
        $env:RUSTUP_TOOLCHAIN = $previousRustupToolchain
    }
    Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
}
