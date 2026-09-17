# SpecGate to CTSC Migration Plan

## Goal

Reframe SpecGate as a deterministic differential-testing system:

```text
reference implementation
  -> discover semantic registry
  -> capture CTSC behavior
  -> statically link the same semantic inputs to another implementation
  -> replay
  -> compare CTSC traces
```

The CTSC registry defines the shared semantic surface. Each implementation
retains its own native signatures, setup methods, object layout, and projection
logic.

## Artifact model

- **Registry**: CTSC components, operations, semantic inputs, observations,
  outcomes, and types.
- **Reference trace**: captured CTSC scenarios and their behavior. It is both
  the replay stimulus and comparison baseline.
- **Discovery metadata**: implementation-specific operations, setup producers,
  receiver construction, native types, and projection information.
- **Semantic map**: optional explicit mappings when automatic linking is
  ambiguous or requires renaming or value transformation.
- **Invocation plan**: a target-local, statically compiled setup/operation DAG.
- **Comparison policy**: deterministic rules such as `ctsc.strict/0.1.0`.
- **Differential report**: mismatches located by scenario, operation path,
  outcome, or observation.

Reference traces provide scenario names, top-level operation order or
parallelism, and projected input values. Nested operations are observed
behavior, not replay instructions. The candidate produces its own nested
operation tree for comparison.

## Phase 1: CTSC compatibility layer

Add a Rust crate that owns:

- CTSC registry models;
- a lightweight CTSC trace/span/event model;
- OTLP JSON encoding and decoding;
- Registry, Trace Core, and Linked validation;
- the `ctsc.strict/0.1.0` comparison policy.

Begin with a compatibility translator from the existing flat trace:

| Legacy trace | CTSC |
|---|---|
| `Run` | `conformance.operation` span |
| `<operation>.<input>` | operation input attribute |
| `$result` | result, empty, or declared-error completion event |
| `$fault` | fault event |
| other events | observation events |

Generated scenarios initially contain ordered sibling operation spans.

Native production capture has now started for synchronous Rust operations.
`specgate-runtime` records explicit operation spans at annotation-generated
invocation scopes, including nested parentage, semantic inputs, observations,
completion, status, and logical timestamps. `specgate-ctsc` encodes those
records directly as CTSC OTLP JSON and exposes no parallel legacy output.
Legacy flat-trace translation remains only for extraction and harness paths
that do not yet have CTSC replacements; it is not a compatibility promise.

## Phase 2: CTSC registry export

Reuse Rust link-time discovery and C# reflection discovery, then replace the
current `DiscoveredSchema` projection with CTSC registry generation.

Discovery metadata must retain:

- result, empty, and declared-error channels;
- observation names and types;
- setup input/output relationships;
- component dependencies;
- stable semantic identifiers.

Keep raw implementation metadata separate from the language-neutral registry.

## Phase 3: Reference capture

The first Rust reference-capture slice is implemented as:

```text
existing tests
  -> isolated execution
  -> environment-activated native operation scopes
  -> CTSC registry
  -> reference OTLP trace
```

`specgate capture <binding.yaml> --out <dir> [--target <name>]
[--component <id>]` runs every libtest test in isolation. Passing tests that
invoke the selected component become scenarios; unrelated and failing tests are
omitted. Runtime sidecars contain native operation trees directly and never
reconstruct them from `TraceEvent`, `SPECGATE_RECORD`, flat event names, or an
extracted spec.

The deterministic capture bundle contains:

```text
capture/
  registry.ctsc.json
  reference.otlp.json
  manifest.json
```

The manifest contains provenance, digests, target identity, and tool version,
but no behavioral expectations.

This slice supports Rust targets only and uses the initial registry version
`0.1.0`. Compatibility-driven registry evolution and additional target
languages remain later work.

## Phase 4: Static semantic linker

Compile a target-local invocation plan from the CTSC registry, target discovery
metadata, and optional mappings:

```text
semantic inputs
  -> setup/constructor calls
  -> receivers and arguments
  -> operation call
  -> outcome and observation projection
```

Automatic linking uses component, operation, input, and type identity. It must
fail deterministically on ambiguous setup producers or parameter mappings.

Explicit mappings cover:

- operation or input renames;
- input composition and decomposition;
- alternative constructors;
- outcome translation;
- ignored or projected observations.

The first linker slice targets synchronous public Rust free functions with
primitive semantic inputs. It requires exact component, operation, input name,
input order, input type, and output type identity. Setups, methods, async
operations, structured values, and explicit mappings remain unsupported until
their invocation-plan nodes are implemented; the linker rejects them rather
than guessing.

**Status:** the first linker slice is implemented. Replay builds a typed,
serializable target-local plan from verified CTSC stimuli plus raw and
normalized link-time discovery metadata before generating any candidate code.

## Phase 5: Candidate replay

Read each reference scenario, select its top-level operations, feed semantic
inputs through the candidate invocation plans, and emit an independent CTSC
trace.

Implement Rust replay first, then route C# through the same language-neutral
plan model.

The initial command surface is:

```text
specgate replay <capture/> <candidate-binding.yaml> \
  --out candidate.otlp.json [--target <name>]
```

Replay reads no `.spec.yaml`. It invokes only top-level operations from the
reference scenarios; nested operations are independently observed from the
candidate.

**Status:** the first Rust replay slice is implemented for lossless CTSC
`unit`, `string`, `bool`, `i32`, `i64`, `u32`, `u64`, `f32`, and `f64`
values. Candidate scenarios are captured natively and encoded as one
deterministic linked run with IDs independent from the reference. Strict
comparison, structured values, setups/methods, async, and additional languages
remain later slices.

## Phase 6: Differential comparison

Implement `ctsc.strict/0.1.0` first:

- scenarios pair by name;
- sequential operations pair by position;
- parallel branches pair by semantic identity;
- operation identities and semantic inputs must match;
- observations and completion states compare structurally;
- values use registry-declared collection and numeric semantics.

Reports must identify the scenario, operation path, event or child position,
and reference/candidate semantic values.

Configurable policies such as ignored observations, numeric tolerances, and
relaxed collection matching come after Strict.

## Phase 7: CLI transition

Target command surface:

```text
specgate discover <target> --out registry.ctsc.json
specgate capture <target> --out capture/
specgate replay <capture/> --target <candidate>
specgate compare <reference.otlp.json> <candidate.otlp.json>
specgate diff <capture/> --target <candidate>
specgate validate <artifact>
```

`diff` combines candidate replay and comparison and becomes the primary
workflow.

## Phase 8: Incremental legacy retirement

Remove each legacy output or subsystem when its CTSC replacement lands:

1. Native capture emits only CTSC artifacts.
2. Convert existing fixtures to capture bundles.
3. Move extraction and self-hosting onto CTSC artifacts, then remove flat trace
   recording.
4. Remove `.spec.yaml` assertion matching and spec-driven runner generation
   after replay and comparison cover those workflows.
5. Remove legacy case, assertion, matcher, and value-projection types when
   their last consumers are gone.
6. Rename spec-oriented annotations and APIs after the CTSC workflow is
   complete; no compatibility layer is required.

## First vertical slice

Use the stateless-add Rust/C# fixture:

```text
existing Rust test
  -> native CTSC operation capture
  -> generated CTSC registry
  -> reference OTLP trace
  -> statically linked C# invocation
  -> candidate CTSC trace
  -> Strict comparison
```

Acceptance criteria:

- Both traces pass CTSC Trace Core validation.
- The registry passes Registry and Linked validation.
- Candidate replay does not read a `.spec.yaml`.
- Changing the C# result produces a deterministic differential report.
- Repeated runs produce semantically equivalent artifacts.

The first implementation milestone covers legacy trace translation for one
stateless operation. Registry export and static linking follow only after that
translation is validated against the CTSC corpus.
