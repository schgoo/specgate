# Named enum replay inputs

> **ADR:** ADR-0028
> **Status:** construction principle accepted; nested setup design pending; not implemented
> **Owner:** @schgoo (construction principle approved 2026-11-20)
> **Date:** 2026-11-18
> **Tracking:** [#158](https://github.com/schgoo/specgate/issues/158),
> [roadmap #159](https://github.com/schgoo/specgate/issues/159)

## Context

**SpecGate can record a named enum input but cannot yet rebuild that value
for replay.**

Why it matters: a valid recording can stop before the candidate operation runs.

- **Example:** `Topology` is an enum, a closed set of choices. `Unknown` needs
  no arguments, `Geography(i32)` needs a number, and
  `Geometry { srid: i32 }` needs a named field. All are constructors.
- **Current limit:** capture records the selected choice and its named type.
  Replay then rejects it with
  `input 'topology' uses unsupported structured replay type named`.
- **Different checks:** bundle validation checks the recording against its
  declarations. Replay must also know how to create the corresponding Rust
  value; passing the first check does not supply that construction code.
- **Construction inputs:** a variant's arguments may come directly from recorded
  data or from an explicit setup producer. A producer may build a payload or
  the whole variant.
- **No guessing:** a recorded result does not reveal a factory's original
  arguments or authorize invented state. Setup selection and the source of
  its construction inputs must be explicit.

## Minimal example

Rust construction excerpt; imports, the crate-root component declaration,
the annotated `Crs` type, and its native `make_crs` factory are omitted:

```rust
#[derive(SpecEvent)]
pub enum Topology {
    Unknown,
    Geography(i32),
    Geometry { srid: i32 },
    Custom(Crs),
}

fn make_custom(epsg: i32) -> Topology {
    Topology::Custom(make_crs(epsg))
}
```

`Unknown` needs no inputs. `Geography(4326)` and `Geometry { srid: 0 }` can use
checked recorded payloads directly. `Custom` may need a setup such as
`make_custom`: recorded construction input `epsg` drives the real native factory.
A setup could instead supply the `Crs` argument to the variant constructor.

The reference and candidate must have corresponding semantic declarations.
A bare reference `Geography` tag cannot supply an unrecorded integer. The
function illustrates setup-backed construction, not new setup annotation or
binding syntax; nested producer selection remains to be designed.

## Decision

**Approved principle (2026-11-20):** support every enum constructor form:
unit, tuple, and named fields. Each constructor has typed inputs. Supply those
inputs from recorded values or explicitly selected setup producers; a producer
may also construct the whole variant. Constructor shape alone is not a reason
to reject replay.

**Still pending:** the binding, recording/provenance, dependency, sharing, and
lifetime model for nested setups, plus any required public API. The approach
below does not ratify those details.

| Boundary | Construction requirement |
|---|---|
| Declaration | Resolve the named enum and require matching semantic type/variant identities and compatible payload declarations. Do not equate integer ordinals with payloads. |
| Direct construction | Decode complete constructor arguments through checked input reconstruction, then call the actual native variant constructor. |
| Setup-backed construction | Use an explicitly selected producer for a payload or the whole variant, with declared construction inputs. Never infer a factory solely from its return type. |
| Missing data | Require an explicit construction rule for omitted native state; recording or ignoring a field alone does not authorize defaulting it. |
| Reconstruction | Generate constructor code in the enum's lexical scope and reuse the generated-harness input mechanism, rather than a special adapter per enum. |
| Dispatch | Use the original Cargo target's operation bridge when native types cannot safely be named by an external runner. |
| Rejection | Reject unknown variants, malformed payloads, incompatible declarations, and unresolved or ambiguous construction paths. Do not invoke the target operation with guessed inputs. |

This replaces the initial unit-only proposal. It does not make every
`SpecEvent` projection automatically reversible. An ignored payload remains
native constructor state; it is not a unit variant. An explicit producer may
own its initialization under the agreed construction contract.

## Consequences

- Unit, tuple, and named-field variants are in scope for operation and
  synchronous setup inputs. Payloads use shared construction capabilities;
  nested enums, records, and containers must not require per-enum adapters.
- Preserve existing explicit producer/property-setup opt-ins and default
  precedence from [ADR-0014](constructor-and-property-setups.md). This does not
  silently enable arbitrary record reconstruction or new omitted-field defaults.
- Existing generic-specialization limits, C# flags (#79), async setups, and
  cross-schema variant mappings need their own scope decisions.
- Reuse existing tagged-union wire shapes. Any required CTSC or public API
  change must stop for owner review.
- Existing Option/Result decoding keeps its own rules; variants named Some,
  None, Ok, or Err do not turn an ordinary enum into a standard wrapper.
- Registry, Trace, Linked, and bundle validity remain separate from supported
  replay construction. No ordinal compatibility is inferred across languages.
- Reject a special Topology-only adapter: the capability must work for any enum
  meeting the same checked boundary.

## Implementation approach (proposed)

1. Extend registry-aware replay decoding and storage checks to carry typed
   variant payloads and their construction requirements.
2. Generate compiler-checked constructor bridges alongside enum metadata.
   Delegate each argument to shared input reconstruction or an explicit producer.
3. Before adding nested producers, review their selection, input capture and
   bindings, dependency order, reuse, teardown, and cyclic-dependency handling.
   Existing whole-operation-parameter producers retain their current rules.

## Validation

Cover unit, single/multiple tuple-field, named-field, and mixed variants through
ordinary capture/replay tests. Use matching C# reference shapes where available
and Rust reference fixtures for constructor shapes C# does not express directly.
Verify actual native arguments, variant identities, and payload round trips;
integer ordinals must not select variants.

Exercise direct reconstruction, payload-producing and whole-variant setups,
and nested enum/record/container payloads with valid construction plans.
Recorded construction inputs must drive the real factory. Test private/test-local
resolution, unknown tags, malformed payloads, incompatible declarations, missing
construction inputs, and ambiguous owners/producers without invented values.
Nested setup ordering, reuse, and teardown tests follow the remaining design
approval. Unit-only support is not completion of this issue.

Implementation seams: registry-aware replay decoding/value model, Rust storage
checks, `SpecEvent` derive/input decoder, and lexical runner dispatch. Add
matrix evidence, regenerate goldens, update input-limit docs, and run focused
Rust/C# checks plus `just check`.

## Supersedes / superseded by

Extends only the structured-input boundary in
[typed replay](typed-async-client-replay.md#canonical-adapters-and-structured-inputs).
The [explicit setup rules](constructor-and-property-setups.md) still govern
records, omitted fields, and invariant-sensitive construction.
