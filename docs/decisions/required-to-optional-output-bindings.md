# Required-to-optional output bindings

> **Status:** proposed; not implemented
> **Owner:** @schgoo (approval requested)
> **Date:** 2026-11-18
> **Tracking:** [#155](https://github.com/schgoo/specgate/issues/155),
> [roadmap #159](https://github.com/schgoo/specgate/issues/159)

## Context

The reported gap is **reference `optional<T>`, candidate required `T`**.
The candidate can supply every concrete `T` without supplying a native optional.
Current checked binding rejects the difference; a handwritten `Some(value)`
projection works but replaces the candidate's declared shape.

## Decision

**Requested:** add one explicit, directional output correspondence:
read candidate `v: T` as `Some(v)` in a derived comparison view. Keep the
candidate trace and registry unchanged under the
[shared native-publication design](operation-scoped-type-bindings.md).

| Reference | Actual candidate | Mapped behavior | Signature |
|---|---|---|---|
| `Some(5)` | required `5` | Equal | Optional versus required remains different. |
| `Some(6)` | required `5` | Payload mismatch | Different |
| `None` | required `5` | Presence mismatch | Different |
| Omitted field | required `5` | Not None; assess absence against the reference's own declaration. | Assess native declarations separately. |

Bind exact result-field paths and check the same payload type recursively.
Do not infer eligibility from captures containing no nulls. Successful value
comparison does not prove that the candidate implements the reference's
nullable API contract.

## Consequences

- First slice: fields and supported nested value slots inside a result.
  Top-level optional completion channels are excluded until separately scoped.
- No reverse optional-to-required unwrapping, input coercion, integer widening,
  default substitution, error-channel conversion, or general wrapper elision.
- Required/optional is still a signature difference under #137. It is not the
  pairwise compatibility rule for `oblivious` source metadata.
- Build on confirmed #131/#132 wrapper semantics, #135 provenance/reports,
  #137 signatures, and #156's shared binding design. Do not import wrapper WIP.
- Reject automatic promotion of every required value: explicit intent keeps
  cross-schema policy visible and prevents accidental input conversion.

## Validation

Test all table rows at a record field and a nested result path. Preserve
missing versus None, `Some(())` versus None, unequal payloads, and full native
registry provenance. Unbound output shapes retain strict rejection.
Reverse bindings and required candidate inputs receiving reference None must
remain unsupported; no actual candidate value is synthesized or replaced.

Implementation seams: correspondence configuration/preflight and typed
comparison, not mutation of native capture. Extend real replay/publication and
cross-schema comparator tests; update value-mapping docs and run `just check`.

## Supersedes / superseded by

Adds a bounded mapped-policy alternative to exact optional storage binding.
It does not change [value decomposition](value-decomposition.md#slot-properties)
or [CTSC Strict](../ctsc/comparison.md)'s existing same-registry semantics.
