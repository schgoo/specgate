$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$fixture = Join-Path ([IO.Path]::GetTempPath()) "sg-rust-build test-$([guid]::NewGuid().ToString('N'))"
$workspace = Join-Path $fixture 'rust'
$previousBuildDir = $env:CARGO_BUILD_BUILD_DIR
$previousTargetDir = $env:CARGO_TARGET_DIR
$previousThreads = $env:RUST_TEST_THREADS
$previousCounter = $env:SPECGATE_REUSE_PROBE_COUNT

try {
    New-Item -ItemType Directory -Path (Join-Path $fixture 'scripts'), (Join-Path $workspace 'src'),
        (Join-Path $workspace '.cargo'), (Join-Path $workspace 'shared/src') | Out-Null
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'rust-test.ps1') -Destination (Join-Path $fixture 'scripts/rust-test.ps1')
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot '../rust/rust-toolchain.toml') -Destination $workspace
    [IO.File]::WriteAllText((Join-Path $workspace 'Cargo.toml'), @'
[package]
name = "workspace-build-reuse"
version = "0.1.0"
edition = "2021"

[workspace]
'@)
    [IO.File]::WriteAllText((Join-Path $workspace '.cargo/config.toml'), @'
[env]
CARGO_BUILD_BUILD_DIR = { value = "must-be-overridden-for-tests", force = false }
'@)
    [IO.File]::WriteAllText((Join-Path $workspace 'src/lib.rs'), @'
/// ```
/// let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
/// std::fs::write(root.join("doc-build-dir"), std::env::var("CARGO_BUILD_BUILD_DIR").unwrap()).unwrap();
/// ```
pub fn documented() {}

#[test]
fn generated_builds_share_only_intermediates() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let build_dir = std::env::var("CARGO_BUILD_BUILD_DIR").unwrap();
    assert!(std::path::Path::new(&build_dir).starts_with(root.join("target")));
    std::fs::write(root.join("test-build-dir"), build_dir).unwrap();
    std::fs::write(root.join("test-executable"), std::env::current_exe().unwrap().to_str().unwrap()).unwrap();
    for candidate in ["candidate-a", "candidate-b"] {
        let project = root.join(candidate);
        assert!(std::process::Command::new(env!("CARGO"))
            .current_dir(&project)
            .args(["build", "--quiet"])
            .env("CARGO_TARGET_DIR", project.join("target"))
            .status().unwrap().success());
    }
}
'@)
    [IO.File]::WriteAllText((Join-Path $workspace 'shared/Cargo.toml'), @'
[package]
name = "shared-build-probe"
version = "0.1.0"
edition = "2021"

[workspace]
'@)
    [IO.File]::WriteAllText((Join-Path $workspace 'shared/src/lib.rs'), 'pub fn shared() {}')
    [IO.File]::WriteAllText((Join-Path $workspace 'shared/build.rs'), @'
use std::io::Write;

fn main() {
    let path = std::env::var("SPECGATE_REUSE_PROBE_COUNT").unwrap();
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    writeln!(file, "compiled").unwrap();
}
'@)
    foreach ($name in @('candidate-a', 'candidate-b')) {
        $project = Join-Path $workspace $name
        New-Item -ItemType Directory -Path (Join-Path $project 'src'), (Join-Path $project '.cargo') | Out-Null
        [IO.File]::WriteAllText((Join-Path $project 'Cargo.toml'), @"
[package]
name = "$name"
version = "0.1.0"
edition = "2021"

[dependencies]
shared-build-probe = { path = "../shared" }

[workspace]
"@)
        [IO.File]::WriteAllText((Join-Path $project '.cargo/config.toml'), @'
[build]
rustflags = ["--cfg", "candidate_config_active"]
'@)
        [IO.File]::WriteAllText((Join-Path $project 'src/lib.rs'), @'
#![allow(unexpected_cfgs)]
#[cfg(not(candidate_config_active))]
compile_error!("Candidate-rooted Cargo configuration was not used.");
pub fn candidate() { shared_build_probe::shared(); }
'@)
    }

    $env:CARGO_TARGET_DIR = Join-Path $fixture 'caller-target'
    $env:RUST_TEST_THREADS = '8'
    $env:SPECGATE_REUSE_PROBE_COUNT = Join-Path $workspace 'dependency-builds'
    $previousScratch = $null
    Push-Location $workspace
    try {
        foreach ($callerBuildDir in @($null, (Join-Path $fixture 'caller-build'))) {
            $env:CARGO_BUILD_BUILD_DIR = $callerBuildDir
            [IO.File]::Delete($env:SPECGATE_REUSE_PROBE_COUNT)
            $messages = @(& cargo build --workspace --all-targets --quiet --message-format=json)
            if ($LASTEXITCODE -ne 0) { throw 'Failed to warm the workspace regression fixture.' }
            $executables = @($messages | ConvertFrom-Json | Where-Object {
                $_.reason -eq 'compiler-artifact' -and $_.profile.test -and $_.executable
            } | Select-Object -ExpandProperty executable -Unique)
            if ($executables.Count -ne 1) { throw 'Expected one workspace regression test executable.' }
            $executable = $executables[0]
            $compiledAt = [IO.File]::GetLastWriteTimeUtc($executable)

            & pwsh -NoLogo -NoProfile -File (Join-Path $fixture 'scripts/rust-test.ps1')
            if ($LASTEXITCODE -ne 0) { throw 'Native Cargo build-reuse regression failed.' }
            if ([IO.File]::GetLastWriteTimeUtc($executable) -ne $compiledAt -or
                [IO.File]::ReadAllText((Join-Path $workspace 'test-executable')) -cne $executable) {
                throw 'The test runner rebuilt or stopped using the warmed workspace executable.'
            }
            $buildDir = [IO.File]::ReadAllText((Join-Path $workspace 'test-build-dir'))
            if ([IO.File]::ReadAllText((Join-Path $workspace 'doc-build-dir')) -cne $buildDir -or
                (Test-Path -LiteralPath $buildDir) -or $buildDir -ceq $previousScratch) {
                throw 'Unit tests and doctests did not share fresh, cleaned child-build intermediates.'
            }
            if (@([IO.File]::ReadAllLines($env:SPECGATE_REUSE_PROBE_COUNT)).Count -ne 1) {
                throw 'Isolated candidate targets compiled their shared dependency more than once.'
            }
            foreach ($name in @('candidate-a', 'candidate-b')) {
                if (-not (Test-Path -LiteralPath (Join-Path $workspace "$name/target/debug"))) {
                    throw 'A generated candidate lost its isolated final target directory.'
                }
            }
            if ($env:CARGO_BUILD_BUILD_DIR -cne $callerBuildDir -or
                $env:CARGO_TARGET_DIR -cne (Join-Path $fixture 'caller-target') -or $env:RUST_TEST_THREADS -cne '8') {
                throw 'The native regression changed caller build, target, or thread settings.'
            }
            $previousScratch = $buildDir
        }
    }
    finally {
        Pop-Location
    }
}
finally {
    $env:CARGO_BUILD_BUILD_DIR = $previousBuildDir
    $env:CARGO_TARGET_DIR = $previousTargetDir
    $env:RUST_TEST_THREADS = $previousThreads
    $env:SPECGATE_REUSE_PROBE_COUNT = $previousCounter
    if (Test-Path -LiteralPath $fixture) {
        Remove-Item -LiteralPath $fixture -Recurse -Force
    }
}

Write-Output 'Native Cargo workspace build-reuse regressions passed.'
