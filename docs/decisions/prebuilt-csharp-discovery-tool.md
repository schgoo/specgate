# Prebuilt C# discovery tool

> **Status:** accepted
> **Owner:** SpecGate maintainers (user-ratified)
> **Date:** 2026-11-17

## Context

C# discovery reflects over a compiled candidate assembly and emits one raw
registry document per requested component. That reflection program lived as a
485-line C# source string embedded in a Rust string literal. At discovery time
the Rust driver wrote the string to disk, generated a `.csproj` around it that
referenced the candidate's own `SpecGate.Annotations.dll` by `HintPath`, and
compiled the result.

The embedded program was never compiled by the repository gate. A syntax
error, a bad overload, or a nullability mistake in it surfaced only when the
CTSC golden harness ran, as a `dotnet build` failure reported through the Rust
driver. Nothing offered completion, analyzers, formatting, or unit tests, and
`just dotnet-build` could not see the code at all. The generated project also
paid a compile on every discovery run.

## Decision

- The reflection program is a real project, `csharp/SpecGate.Discovery`, built
  with the rest of the C# solution and covered by `just dotnet-build`,
  `just dotnet-test`, and `just format-check-cs`.
- The Rust driver does not generate, scaffold, or compile C# source. It
  locates the prebuilt assembly and runs
  `dotnet <dir>/SpecGate.Discovery.dll <output-dir> <assembly> <assembly-out-dir> [component...]`.
  Positional arguments replace the previous text substitution, and the output
  filenames are unchanged.
- The tool is located beside the running `specgate` executable.
  `SPECGATE_DISCOVERY_PATH` overrides that with either the containing
  directory or the assembly path. Both candidates are existence-checked and
  failures name the path that was searched.
- The tool reads annotations by `Type.FullName` and does not reference
  `SpecGate.Annotations`. A prebuilt tool and a candidate assembly carry
  independent copies of the annotations package, so attribute reads must not
  depend on the two copies resolving to one `Type` identity. Name matching
  mirrors what the embedded program already did for `SpecEventAttribute`. The
  name is matched against `CustomAttributeData`, which names attribute types
  without constructing them, and only the resolved annotation type is then read
  through the inheritance-aware `Attribute` API.
- Discovery emits no CTSC artifacts. Normalization, registry encoding, and
  manifests stay in Rust, and the Rust CLI remains the only entry point.

## Consequences

- A C# mistake is now a build failure in the ordinary C# gate, not a golden
  failure reported through Rust.
- `SpecGate.Discovery.Tests` drives `DiscoveryRunner.Run` through the
  `RunnerSystem` adapter with an in-memory filesystem.
- Nothing sits beside the executable in a source checkout, so
  `just discovery-tool` builds the tool into `artifacts/discovery` and every
  recipe that drives C# discovery exports `SPECGATE_DISCOVERY_PATH`. A bare
  `cargo test` that reaches C# discovery fails with an error naming the
  variable.
- A release layout must ship `SpecGate.Discovery` beside the `specgate`
  executable. The release workflow uploads that layout per platform and runs a
  C# discovery against it.
- Discovery no longer compiles the reflection program per run; only the
  candidate assembly is built.

## Validation

- `SpecGate.Discovery.Tests` covers argument rejection, the component
  inventory, positional component output, unscoped setup scoping, semantic and
  raw metadata, enum variant declaration order, inherited parameter input
  names, and an unrelated attribute whose constructor throws.
- `specgate-discovery` unit tests pin tool resolution and the exact process
  request, including the missing-assembly failure and a relative override.
- The CTSC goldens are the port's correctness proof: regenerating them after
  the move produced a byte-identical tree.

## Alternatives rejected

- **Ship the tool as a NuGet package.** There is no publish pipeline for it,
  and restoring it would put the network on the discovery path.
- **Name the tool in the binding file.** The binding is a user contract that
  describes the candidate, not the location of SpecGate's own tooling.
- **Parse ECMA-335 metadata from Rust.** Nullability decoding and generic
  instantiation would have to be reimplemented against the compiler's encoding,
  with no reuse of `NullabilityInfoContext`.
- **Invoke the platform apphost `SpecGate.Discovery.exe`.** Running the
  assembly through `dotnet` keeps one resolution rule on every operating
  system.
