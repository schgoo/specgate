# Named enum replay inputs

> **Status:** proposed; not implemented
> **Owner:** @schgoo (approval requested)
> **Date:** 2026-11-18
> **Tracking:** [#158](https://github.com/schgoo/specgate/issues/158),
> [roadmap #159](https://github.com/schgoo/specgate/issues/159)

## Context

A named enum input can form a valid captured bundle but fail replay decoding
and Rust storage checks. Bundle validity does not promise that SpecGate can
construct every declared native input. Output projection alone is generally
insufficient evidence for reconstructing arbitrary records or unions.

## Decision

**Requested:** support lossless reconstruction of complete payload-free enums,
without new annotation syntax or a new CTSC wire representation.

| Boundary | Rule |
|---|---|
| Reference | Resolve the named declaration and require a closed tagged union whose variants have no payload. |
| Candidate | Require a derived, nongeneric Rust enum with only unit variants and matching semantic type/variant identities. |
| Value | Retain the semantic variant name and unit payload; never use an integer ordinal, default, or contents-based inference. |
| Reconstruction | Derive a compiler-owned decoder in the enum's lexical scope; use the existing generated-harness input mechanism. |
| Dispatch | Use the original Cargo target's operation bridge when native types cannot safely be named by an external runner. |
| Rejection | Unknown variants, invalid payloads, ambiguous owners, and incompatible declarations fail before invocation. |

Automatic reconstruction applies only when derive expansion proves that every
variant is a unit variant and the representation is complete. It does not make
all `SpecEvent` types reconstructable. A projected/ignored payload is not a
payload-free variant and must not qualify.

## Consequences

- First slice: direct enum-valued operation and synchronous setup inputs with
  matching identities. Containers of named inputs, payload unions, general
  records, flags (#79), and cross-schema variant mappings remain excluded.
- Existing Option/Result decoding keeps its own rules; variants named Some,
  None, Ok, or Err do not turn an ordinary enum into a standard wrapper.
- Registry, Trace, Linked, and bundle validity remain separate from supported
  replay construction. No ordinal compatibility is inferred across languages.
- Reject a special Topology-only adapter: the capability must work for any enum
  meeting the same checked boundary.

## Validation

Capture an ordinary C# operation with a named two-variant input and replay both
variants against a Rust candidate. Verify actual candidate behavior and input
round trips, private/test-local lexical resolution, exact names despite
different native ordinals, malformed tags, and rejected payload variants.

Implementation seams: registry-aware replay decoding/value model, Rust storage
checks, `SpecEvent` derive/input decoder, and lexical runner dispatch. Add
matrix evidence, regenerate goldens, update input-limit docs, and run focused
Rust/C# checks plus `just check`.

## Supersedes / superseded by

Extends only the structured-input boundary in
[typed replay](typed-async-client-replay.md#canonical-adapters-and-structured-inputs).
The [explicit setup rules](constructor-and-property-setups.md) still govern
records, omitted fields, and invariant-sensitive construction.
