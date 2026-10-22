# Behavior fixtures

Start with a component in [`matrix.json`](matrix.json), then open
`cases/<existing-component-id>/`. A case contains its real Rust implementation
and ordinary native tests, its C# implementation and `csharp/tests/` assertion
steps where applicable, and one shared native bundle in `expected/`. Rust-only
cases have no artificial C# directory.

## Where things live

- `cases/`: behavior, not build projects. Native tests call the real annotated
  operations; CTSC capture observes them without a separate assertion format.
- `projects/rust/`: the main aggregate fixture package and the separate
  `specgate-facade-umbrella` package. The latter deliberately imports the facade
  as `sg`, proving renamed-dependency resolution. [`test/Cargo.toml`](../Cargo.toml)
  is the positive fixture workspace. The module aggregators preserve existing
  Rust module paths and scenario identities while compiling the moved sources.
- `projects/csharp/`: one implementation assembly and one test assembly.
  Explicit compile includes keep case tests out of the implementation assembly.
  `MetadataTests.cs` lives in `../mechanics/csharp/SpecGate.CtscFixtures.Tests`
  but remains compiled into the same behavior test assembly.
- `../mechanics/csharp/`: runtime, discovery, and IL-weaver product mechanics,
  with their ordinary tests and isolated woven fixtures. The byte fixture at
  `../mechanics/capture-sidecar/parity.json` is a protocol contract, not a
  behavior bundle.
- `../rejection/`: the isolated Rust negative package and separate C# weave
  rejection assemblies. Their isolation preserves first-error coverage.
- `../support/csharp/`: shared native test support.

## Shared scenarios and discovery-only sources

A shared scenario is still one original Fact or Rust test. Partial C# classes
place unannotated assertion steps beside their cases; the original Fact owner
calls them in the original order. Case notes link these owners. They do not
introduce additional enumerated tests. `OptionResult.Locate` belongs to
`fixture.option_some`; `OptionResult.Divide` belongs to `fixture.result_ok`.
Sibling partials use those helpers without wrappers or duplicate code.

Rust's `registry::component` aggregator includes the `comp.core` and `comp.app`
fragments in the same original module. The shared timer scenario lives with
`fixture.async_smol_timer` and exercises smol before Tokio; both fragments remain
in `registry::async_metadata`. The strings implementation stays at
`fallible_unit::strings` although its physical source belongs to `fixture.strings`.

Source accounting uses the matrix's existing roots, sources, and explicit
exclusions. The main crate's component-only annotation is an explicit
`declares-no-operation` exclusion; unannotated aggregators and helpers are
out of scope by the `no-spec-annotation` rule. Discovery-only endpoints do not
imply native capture: in particular Rust `fixture.fallible_unit` retains its
observation limitation while C# supplies the selected native bundle.

## Generated ownership

Only the matrix's `artifactDirectory` paths own generated output:

- `cases/<component>/expected/{manifest.json,reference.otlp.json,registry.ctsc.json}`;
- `../rejection/rust/specgate-ctsc-negative-fixtures/expected/<category>/rust/error.json`.

The matrix retains stable row IDs independently of layout. The selected native
producer writes directly to `expected/`, truthfully recording `target.name` as
`expected`. Other producer registries, strict native captures, and replay traces
remain independent scratch outputs, validated against their own registries.
Check mode anchors the selected full registry to canonical checked-in bytes.

Never edit or move generated artifacts by hand. Run:

```powershell
just ctsc-goldens-update
just ctsc-goldens-check
```

Before any mutation, the harness validates ownership and the complete reserved
namespace, including orphan/empty `expected/` directories, unknown files,
nested language/reference bundles, candidate leaks and misplaced recognized
golden names. Output roots, their ancestors, and inventory descendants must not
be symlinks or Windows junctions, even when the output directory is missing.
Safely missing directories remain valid for generation.
Unknown material fails; update never cleans it up. Ordinary
handwritten files outside `expected/` are preserved. Build outputs are ignored
only beside actual build projects, never inside generated output.

## Adding a case

1. Choose the semantic component ID and add real source under that case's
   applicable language directory. Wire Rust into the existing aggregate module
   tree without changing module identity; C# case sources/tests use the existing
   aggregate compile includes. Do not create a package/project per case.
2. Add ordinary native test coverage. Reuse a shared scenario where appropriate,
   keeping its single entry-point owner and documenting cross-case ownership.
3. Add the matrix row, source declarations, binding implementation and layout-only
   artifact directory. Declare all phases, exclusions, exact parity differences,
   and directed replay checks honestly; unsupported is not equivalence.
4. Run focused harness and native tests, regenerate and inspect the bundle, then
   run `just check`. `just format[-check]` also reaches included Rust fragments.
