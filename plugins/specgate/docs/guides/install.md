# Install SpecGate

## Goal

Get a working `specgate` command, plus the annotation package for each language
you instrument.

## Prerequisites

- Rust targets: a Rust toolchain with `cargo`, version 1.93 or later (the
  workspace `rust-version` in `rust/Cargo.toml`).
- C# targets:
  - The .NET 10 runtime, to run the C# discovery tool (`SpecGate.Discovery`
    targets `net10.0` and ships framework-dependent). Building it from source
    needs the .NET 10 SDK.
  - An SDK that can build and test your projects, which must target .NET 8 or
    later. Capture builds and runs your tests with `dotnet`.

## Steps

### 1. Get the CLI

Pick one option.

**From a release build.** Each `v*` tag runs
[`release.yml`](https://github.com/schgoo/specgate/blob/v0.6.0/.github/workflows/release.yml), which builds an install
layout for `linux-x86_64`, `windows-x86_64`, and `macos-aarch64`. The layouts are
uploaded as workflow artifacts named `specgate-<os>-<arch>` on the release
workflow run, not as GitHub release assets. Each layout is one directory:

| File | Purpose |
| --- | --- |
| `specgate` / `specgate.exe` | The CLI. |
| `SpecGate.Discovery.dll` and its `.runtimeconfig.json` / `.deps.json` | C# discovery tool. |
| `SpecGate.Weaver.dll`, `SpecGateCapture.targets`, `SpecGateWeave.targets` | C# capture weaver and its MSBuild hooks. |
| `SpecGate.Runtime.dll` (net8.0) | C# capture runtime, added to the woven build automatically. |

The weaver and discovery tool also ship their dependencies (for example
`Mono.Cecil*.dll`).

Keep the files together and put the directory on `PATH`. The CLI finds the C#
tools beside its own executable.

**From crates.io.** Releases publish the CLI as `specgate-cli`:

```sh
cargo install specgate-cli
```

This installs only the `specgate` executable. That is enough for Rust. For C#,
also build the tools (below) and point `SPECGATE_DISCOVERY_PATH` and
`SPECGATE_WEAVER_PATH` at them.

**From source.** In a clone of this repository:

```sh
cd rust
cargo build --release -p specgate-cli
cd ..
just discovery-tool   # builds into artifacts/discovery
just weaver-tool      # builds weaver + net8.0 runtime into artifacts/weaver
```

Then either copy the contents of `artifacts/discovery` and `artifacts/weaver`
beside `rust/target/release/specgate`, or set `SPECGATE_DISCOVERY_PATH` and
`SPECGATE_WEAVER_PATH` to those directories. The `justfile` exports both
variables for its own recipes.

### 2. Add the Rust annotations

In the crate you want to capture:

```sh
cargo add specgate
```

`specgate` is the only crate you depend on; it re-exports the macros and the
runtime.

### 3. Add the C# annotations

In the project that holds your operations (not the test project):

```sh
dotnet add package SpecGate.Annotations
```

The package holds only attributes and the `Option<T>` / `Result<T, E>` types.
Do not reference `SpecGate.Runtime`; capture adds it to the build itself.

## Expected output

`specgate --help` prints the usage text to stderr and exits 0. The text is
reproduced in [`../reference/cli.md`](../reference/cli.md#usage).

## Artifacts

None. Capture and replay scratch builds go under the operating-system cache
directory (override with `SPECGATE_CACHE_DIR`); see
[`../reference/cli.md`](../reference/cli.md#environment-variables).

## Failure modes

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `C# discovery requires SpecGate.Discovery.dll beside the specgate executable` | The discovery tool is missing. | Use a full install layout or set `SPECGATE_DISCOVERY_PATH`. |
| `SPECGATE_DISCOVERY_PATH names no SpecGate.Discovery.dll at` | The override points at the wrong place. | Point it at the directory or the `.dll`. |
| `C# capture requires SpecGateCapture.targets beside the specgate executable` | The weaver layout is missing. | Use a full install layout or set `SPECGATE_WEAVER_PATH`. |
| `no operating-system cache directory is available; set SPECGATE_CACHE_DIR` | No cache root could be found. | Set `SPECGATE_CACHE_DIR`. |

## Verify

```sh
specgate --help
```

Exit code 0. Then run the [agent quick path](README.md#agent-quick-path) on a
small component.
