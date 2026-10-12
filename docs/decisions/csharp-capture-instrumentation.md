# C# capture instruments the built assembly by IL weaving

> **Status:** accepted
> **Owner:** SpecGate maintainers (user-ratified)
> **Date:** 2026-10-12

## Context

C# contributes registry discovery and nothing else. `csharp/SpecGate.Annotations`
is the entire C# source surface: inert attributes plus `Option<T>` and
`Result<T,E>` for metadata. There is no recording code. The former
`SpecGate.Runtime` and `SpecGate.Weaver` were deleted in `63c3716` as part of
legacy CTSC cleanup, not because the approach failed.

Everything around the gap already exists. 42 of the 72 golden-matrix rows carry
a C# registry, and `fixture.async_fetch` is byte-identical with Rust, so C#
*extraction* already handles async. The annotated C# corpus has ordinary xUnit
behavior tests that pass today. The missing piece is something that observes
those tests while they run and emits CTSC spans.

[`../roadmap.md`](../roadmap.md) lists the instrumentation mechanism as an open
implementation choice. This record closes it.

## Decision

1. **Instrumentation is IL weaving of the target's real built assembly**, via an
   MSBuild task over a CIL rewriter, restoring the shape the deleted
   `SpecGate.Weaver` had. The weaver injects entry, normal-return, and throw
   recording into the annotated method's own body.

   The alternatives were rejected on semantics, not effort:

   - **Roslyn source generators cannot rewrite an existing method body.** They
     are additive only. Using one would require annotated methods to become
     `partial` or be renamed, so the target's ordinary source would have to
     change to be capturable. SpecGate captures *ordinary* code through
     *ordinary* tests, per [`../digests/llm.md`](../digests/llm.md).
   - **C# 12 interceptors record the call site, not the callee.** Only callers
     compiled with the generator would record, so a nested internal call or a
     third-party caller is invisible. Rust `#[spec_operation]` rewrites the
     method, so every caller records. Adopting interceptors would make C# and
     Rust capture structurally different surfaces, which defeats the
     cross-language parity the product exists to provide.
   - **The CLR Profiler API** gives correct callee-side semantics but requires a
     native per-platform agent, which is disproportionate here.

   Weaving is the only mechanism whose capture semantics match Rust.

2. **The C# recording runtime is async-capable from its first commit.** Logical
   context flows across `Task` boundaries through `AsyncLocal` or an equivalent,
   even while the first instrumented fixture is synchronous. This follows
   [`../roadmap.md`](../roadmap.md), which already rejects a synchronous-only
   stage: every documented consumer is async C#, and a thread-affine capture
   state would have to be torn out again. The Rust milestone ordering makes the
   same argument — moving thread-affine state must precede enabling async, never
   follow it.

   Synchronous-first applies to *which fixture is instrumented first*, not to
   the runtime's concurrency model.

3. **`SpecGate.Annotations` stays metadata-only.** Recording state and CTSC
   operation scopes live in a separate runtime package, so discovery keeps
   reflecting over attributes that carry no behavior.

4. **C# golden-matrix rows may declare trace artifacts and the capture phase.**
   `goldens/matrix.rs` currently asserts C# coverage is registry-only and
   discovery-only, failing any row that declares more. That assertion is relaxed
   to permit captured C# artifacts. It is a guardrail, so relaxing it must not
   leave a row free to over-claim: a row declaring a C# trace must produce and
   byte-match one, exactly as Rust rows do.

5. **Issue #43 is not a prerequisite.** Exact seam selection is language-neutral
   operation-level filtering, and `specgate capture --component` already selects
   at the granularity the first C# slice needs. #43 remains wanted and is
   sequenced independently rather than blocking C# capture.

## Consequences

- C# and Rust capture record at the same boundary, so registry and trace parity
  stays meaningful and the existing paired fixtures remain the corpus.
- Weaving reintroduces build-integration cost: isolated `bin`/`obj` paths, PDB
  rewriting for debuggable instrumented assemblies, and an MSBuild task that
  must be transparent when recording is inactive.
- The epic [#48](https://github.com/schgoo/specgate/issues/48) and its children
  are no longer scoped synchronous-only; async C# capture moves out of their
  deferred lists.
- `specgate capture` must accept non-Rust bindings, which it currently rejects
  with "only Rust targets".

## Validation

Deferred to the implementing slices. The first slice's acceptance is one
annotated C# fixture captured end to end from its ordinary xUnit test into a
bundle that passes native Registry, Trace Core, Linked, and bundle-integrity
validation, with instrumentation proven behaviorally transparent when inactive.
