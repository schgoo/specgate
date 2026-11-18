# Missing candidate output fields

> **Status:** proposed; not implemented
> **Owner:** @schgoo (approval requested)
> **Date:** 2026-11-18
> **Tracking:** [#154](https://github.com/schgoo/specgate/issues/154),
> [roadmap #159](https://github.com/schgoo/specgate/issues/159)

## Context

The reference record may expose fields absent from the candidate's real API.
Exact storage binding correctly rejects this today. Filling missing fields
with empty strings permits execution but hides API absence as value mismatch.

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
