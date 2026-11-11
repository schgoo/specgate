# Compiler-directed custom collection metadata

> **Status:** accepted direction; not implemented
> **Owner:** @schgoo
> **Date:** 2026-11-11

## Context

Custom Rust collections need semantic metadata without a built-in mapping for
every container or registrations for every concrete specialization. The
[collection proposal](../contributor/custom-collection-contract.md) originally
promised to leave existing traits unchanged. An isolated executable experiment
demonstrated a route that instead extends `SpecEvent`.

## Decision

Extend the existing `SpecEvent` trait with a static metadata method. Generate
implementations through the record/union derive and the single `spec_collection`
attribute. Operation metadata carries compiler-instantiated callbacks, so Rust
resolves aliases, scopes, generics, and borrowing rather than a string-name
lookup. Add supported standard-type metadata through the same contract.

The experimental `__specgate_type_metadata() -> Option<String>` with
`Self: Sized` demonstrates the direction, not a final method name or metadata
representation. Production metadata must retain named-type ownership, including
component and registry qualifications. Preserve object safety and review new
standard/reference implementations for downstream coherence.

Keep manual `ToNativeValue` independent; discovery must not serialize or
construct a value. Do not add another author-facing type trait, per-concrete-
collection registrations, a discovery-only `'static` bound, or reconstruction
permission. Renaming `SpecEvent` remains separate.

Duplicate map keys, duplicate declared-unique elements, and custom-serializer
shape mismatches permanently stop capture as annotation errors regardless of
origin. Preserve conflicts until diagnostics identify the path and offending
value; never silently deduplicate. Bag multiplicity remains valid.

Align with [#126](https://github.com/schgoo/specgate/pull/126): capture/replay
perform structural trace validation, with these narrow annotation checks as
explicit exceptions. Do not run broad linked validation there. General
product-value checking belongs to `compare` and `validate linked`; the golden
gate stays linked and strict.

## Consequences

`SpecEvent` is no longer only a record/union marker. A default unavailable
metadata result can preserve empty marker implementation syntax but must fail
closed when discovery needs that metadata. This is not a blanket compatibility
guarantee.

Named records remain named references inside structural collection/map shapes.
External references reuse existing CTSC imports, component dependencies, and
version/digest checks. Production discovery must generate ownership correctly;
registry-validator support alone does not establish that integration.

Implement the coordinated breaking native-sidecar migration before the
collection extension. No old-sidecar compatibility reader is required. Preserve
representations still used by records, tuples, optionals, and outcomes. CTSC
wire shapes, construction opt-ins, and comparison rules do not change here.

## Validation

The isolated experiment passed six tests, a compile-fail doctest, Clippy,
formatting, and three diagnostic-checked compiler probes. It used the real
operation macro and registry encoder, but a test bridge rather than production
discovery consumed the callbacks. It did not capture or replay a collection
trace. Its full gate did not pass because of disk exhaustion and smoke linker
errors; that experiment is not part of this checkout.

The checked-in
[`collection_metadata_proof.rs`](../../rust/crates/specgate-ctsc/tests/collection_metadata_proof.rs)
separately tests substitution, structural registry encoding, and explicitly
qualified cross-component/imported record references. It registers declarations
by hand; it does not prove automatic discovery. Production evidence required
before shipping is listed in the [proposal](../contributor/custom-collection-contract.md#required-evidence).

## Supersedes / superseded by

Supersedes the proposal's earlier unchanged-`SpecEvent` and temporary
duplicate-rejection promises. Detailed API naming and representation remain
under review.
