# Missing candidate output fields

> **ADR:** ADR-0024
> **Status:** proposed; not implemented
> **Owner:** @schgoo (approval requested)
> **Date:** 2026-11-18
> **Tracking:** [#154](https://github.com/schgoo/specgate/issues/154),
> [roadmap #159](https://github.com/schgoo/specgate/issues/159)

## Context

**A field the candidate does not provide must not look like a field returning
the wrong value.**

Why it matters: shared fields may be correct even when the replacement exposes
fewer fields than the reference implementation.

- **Example:** the C# `CoordinateSystem` record has `EpsgId`, `Id`, and `Name`.
  The Rust candidate may provide only `EpsgId`.
- **Current limit:** replay checks both output definitions before execution.
  Different field sets correctly fail that compatibility check.
- **Misleading workaround:** supplying `""` for `Id` and `Name` lets replay
  proceed, but comparison reports wrong strings instead of missing fields.
- **Important distinction:** never declaring `Name` differs from promising
  `Name` but omitting its recorded value. Neither case means an empty string,
  a default value, or an explicit no-value result such as `None`.

## Minimal example

Output-type excerpts from the same component; imports and operations are omitted.
C# reference:

```csharp
[SpecEvent("CoordinateSystem")]
public sealed record CoordinateSystem(
    [property: SpecEvent] int EpsgId,
    [property: SpecEvent] string Name);
```

Rust candidate:

```rust
#[derive(SpecEvent)]
pub struct CoordinateSystem {
    #[spec_event(name = "EpsgId")]
    pub epsg_id: i32,
}
```

For reference `new CoordinateSystem(4326, "WGS 84")` and candidate
`CoordinateSystem { epsg_id: 4326 }`, an explicit unavailable-field rule for this
result's `Name` would let `EpsgId` compare equal while reporting missing `Name`.
Do not add `Name: ""` to the candidate. The rule's configuration syntax is not
defined here; full API equivalence is not established.

## Decision

**Requested:** allow explicitly declared unavailable output paths through the
[shared operation-scoped correspondence plan](operation-scoped-type-bindings.md).
Publish native declarations and values; never insert missing-value sentinels.

| Observed/declaration state | Finding |
|---|---|
| Both sides declare and return a corresponding field | Compare actual values under its checked type binding. |
| Reference declares a field; candidate API does not | Report the native signature difference and missing candidate field. This alone is not a violation of the candidate's own registry. |
| Candidate declares a field but omits its value | Report its own `missing_member` violation under #134; do not repair the value. |
| Candidate returns None, empty string, unit, or a default | Preserve that present value; it is not an absent field. |
| Candidate has an additional field | Retain and report it; a missing-field declaration must not hide extra native API surface. |

A binding names the exact reference output field that is unavailable.
Preflight checks that it exists in the reference declaration and is absent
from the candidate declaration. If the candidate later adds it, the stale
unavailability rule must reject until updated, not suppress the new field.

Compare all shared fields even when other fields are unavailable. A shared
value mismatch remains visible alongside absence findings. No result with
missing reference fields establishes full API equivalence.

## Consequences

- Scope is output records and supported nested result fields, not missing
  inputs, object reconstruction, implicit ignore, or new CTSC value kinds.
- A complete candidate value can be valid against its own narrower registry.
  Do not mislabel schema difference as malformed observed behavior.
- Reuse #134 violation codes, #135 reports/provenance, #137 signature findings,
  and #156 native publication/bindings. Do not weaken linked validation locally.
- Reject default-filling and projection to only the shared fields: both discard
  evidence needed to explain incomplete candidates.

## Validation

Reference `{id: 7, note: None}`, candidate schema/value `{id: 7}`: compare `id`,
report missing `note`, and report no candidate-own violation for that absence.
Changing candidate `id` to `8` must add a value mismatch. A candidate declaring
`note` but omitting it must instead have an own-registry violation.

Also test nesting, present empty/default/unit values, candidate-only fields,
stale bindings, and original artifact preservation. Extend planner, native
publication, and two-registry comparison tests; run `just check`.

## Supersedes / superseded by

Extends exact-field output binding only through the separately identified mapped
policy in [operation-scoped bindings](operation-scoped-type-bindings.md).
CTSC Strict's equal-declared-fields requirement is not relaxed.
