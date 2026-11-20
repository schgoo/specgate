# Input and output type compatibility

<a id="required-to-optional-output-bindings"></a>

> **ADR:** ADR-0025
> **Status:** Rust `From` input direction accepted 2026-11-20; output mapping proposed; not implemented
> **Owner:** @schgoo (input direction approved; output approval requested)
> **Date:** 2026-11-18
> **Tracking:** [#155](https://github.com/schgoo/specgate/issues/155),
> [roadmap #159](https://github.com/schgoo/specgate/issues/159)

## Context

**Inputs must be constructed before the candidate runs. Outputs are compared
after the candidate runs. Different types need separate rules for each step.**

For Rust inputs, an existing `B: From<A>` implementation provides a conversion
from a constructible recorded value `a: A` to the candidate's required type `B`.
The owner approved using that conversion automatically, without registration
or another opt-in. This does not make the native types equivalent.

For outputs, an optional number and a required number can hold the same value
while making different promises. Compare the actual numbers without pretending
the candidate can also return no number:

- **Example:** reference C# `EpsgId: int?` permits a number or `null`. Candidate
  Rust `i32` requires a number. The direction is **reference `optional<T>`,
  candidate required `T`**, not the reverse.
- **Current limit:** matching field names is not enough. Replay rejects the
  different declared types before comparing their values.
- **Workaround:** a handwritten adapter returns `Some(value)`, meaning a
  present value. That exposes a converted shape instead of the required field
  the candidate actually declares.
- **Hard case:** reference `None`, meaning no value, must differ from candidate
  `5`. Captures containing no nulls do not prove the reference can never
  return null.

## Minimal example

### Inputs: compiler-checked Rust conversion

Illustrative generated call; this helper is not a new public API:

```rust
fn convert_input<A, B: From<A>>(recorded: A) -> B {
    <B as From<A>>::from(recorded)
}
```

For a recorded `4326_i32` and candidate input `i64`, the compiler confirms
`i64: From<i32>`, so replay would convert the value before calling the candidate.
This is approved behavior, not implemented support.

### Outputs: optional reference, required candidate

Output-type excerpts from the same component; imports and operations are omitted.
C# reference:

```csharp
[SpecEvent("CoordinateSystem")]
public sealed record CoordinateSystem(
    [property: SpecEvent] int? EpsgId);
```

Rust candidate:

```rust
#[derive(SpecEvent)]
pub struct CoordinateSystem {
    #[spec_event(name = "EpsgId")]
    pub epsg_id: i32,
}
```

With an explicit binding at this result's `EpsgId` field, reference
`new CoordinateSystem(4326)` could match candidate
`CoordinateSystem { epsg_id: 4326 }`. Only the comparison view reads the
candidate field as `Some(4326)`; its native declaration and value stay required.
Reference `new CoordinateSystem(null)` must still mismatch. The signature
difference remains, and binding syntax is not defined here.

## Decision

### Inputs (accepted 2026-11-20)

For a constructible recorded input `a: A` and candidate input type `B`,
automatically accept and use the conversion when the Rust compiler confirms
`B: From<A>`. No separate converter registration or per-mapping opt-in is
required. This refers to the standard `From` trait, not an arbitrary inherent
method named `from`.

Rust provides `From<i32> for i64`, but not `From<Option<i32>> for i32` or
`From<f64> for i32`. User-defined implementations for eligible types also
qualify; their authors own the conversion behavior. The trait's presence is
the acceptance rule, not proof that custom code preserves information or meaning.

If the source value cannot be constructed or the required implementation is
absent, the unsupported mapping remains rejected; users can provide wrappers.
Do not infer casts, rounding, truncation, or `None` defaults, or automatically
fall back to `TryFrom`. Conversion failures follow ordinary execution rules,
not scenario skipping.

Existing checked storage bindings, setup producers, and explicitly opted-in
default rules remain unchanged. Conversion permits input construction; it does
not erase native type or signature differences. Source-type construction and
native-publication integration still require implementation.

#### Native evidence for input conversions (accepted 2026-11-20)

Replay already runs candidate discovery. Preserve that candidate's native
registry and actual candidate inputs; validate its trace against its own
registry. Compare reference and candidate declarations separately from observed
behavior. Link the native artifacts without rewriting the reference bundle.

This direction does not approve output mapping or a new CTSC Strict equality
policy. Native publication/linking and CLI reports remain pending. #164
implements only the registry-comparison library foundation of #137; #161 still
owns the complete `From` replay feature. Coordinate publication and report
integration with #135.

### Outputs (proposed)

With an explicit mapping for a selected output field, compare the candidate's
actual required value `v: T` as the present optional value `Some(v)`. This is a
comparison-only view: do not change the candidate's recorded value or declared
type. Keep its native trace and registry unchanged under the
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

- Input conversion and output comparison are separate rules with separate
  approval states. Input `From` use is automatic; output mapping needs explicit
  intent and remains proposed.
- First output slice: fields and supported nested value slots inside a result.
  Top-level optional completion channels are excluded until separately scoped.
- The output rule adds no reverse optional-to-required unwrapping, input
  conversion, integer widening, default substitution, error-channel conversion,
  or general wrapper elision. The accepted input rule separately permits
  compiler-confirmed `From` conversions, including supported integer widening.
- Required/optional is still a signature difference under #137. It is not the
  pairwise compatibility rule for `oblivious` source metadata.
- The output design builds on confirmed #131/#132 wrapper semantics,
  #135 provenance/reports, #137 signatures, and #156's shared binding design.
  Do not import wrapper WIP.
- Reject automatic promotion of every required output value: explicit intent
  keeps the output comparison policy visible. An input `From` implementation
  does not authorize an output mapping or establish full API equivalence.

## Validation

### Inputs

Exercise real replay with a standard `From<i32> for i64` conversion and an
eligible user-defined implementation, without converter registration or
per-mapping opt-in. Reject missing implementations, including required `i32`
inputs receiving reference `None` or `f64`. Reject source shapes that cannot
be constructed even if a conversion implementation exists. Test conversion
execution failure through ordinary failure handling, not scenario skipping.
Preserve existing checked storage/setup/default behavior and native
type/signature differences.

Implementation seams: typed source construction, compiler-checked generated
input calls, and native replay/publication integration.

### Outputs

Test all table rows at a record field and a nested result path. Preserve
missing versus None, `Some(())` versus None, unequal payloads, and full native
registry provenance. Unbound output shapes retain strict rejection.
Reverse output bindings remain unsupported; no actual candidate output value
is synthesized or replaced.

Implementation seams: output correspondence configuration/preflight and typed
comparison, not mutation of native capture. Extend real replay/publication
and cross-schema comparator tests for the respective rules; update
value-mapping docs and run `just check`.

## Supersedes / superseded by

Records automatic compiler-confirmed Rust input conversion and proposes a
bounded mapped-policy alternative to exact optional output storage binding.
It does not change [value decomposition](value-decomposition.md#slot-properties)
or [CTSC Strict](../ctsc/comparison.md)'s existing same-registry semantics.
