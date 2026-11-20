# Constructor and property setups

> **ADR:** ADR-0014
> **Status:** accepted
> **Owner:** @schgoo (user-ratified)
> **Date:** 2026-10-23

## Decision

Setup producers are the semantic mapping boundary for native input objects.
C# constructors may declare `[SpecSetup]`: their parameters are construction
inputs and their declaring type is the produced value.

Property setups require an explicit type-level opt-in in each language.
Rust uses `#[derive(SpecEvent)]` with `#[spec_event(setup)]`; C# uses
`[SpecEvent(Setup = true)]`.
Participating fields become flat operation inputs in declaration order. With no
field annotations, all fields participate; otherwise only annotated fields
participate. Existing semantic field names resolve naming differences.
Duplicate folded input names are rejected, never prefixed or overwritten.
Explicit producers take precedence for the parameters they fill.

The [ignored-parameter rule](value-decomposition.md#annotations) now applies
to Rust operations and synchronous setup producers as well as C# parameters.
Ignored parameters are excluded before setup matching, property decomposition,
and provenance comparison; an explicit fill of an ignored-only target rejects
as unknown. Rust replay supplies native `Default` values only at invocation.
Rust ignored `SpecEvent` fields follow the reconstruction precedence below;
the C# member slice does not add reconstruction or defaulting.

For C#, the owner-approved [inherited-member rule](value-decomposition.md#inherited-member-selection-127-owner-approved-2026-11-12)
selects effective public instance members across the hierarchy before applying
the annotation/default rule above. Overrides and `new` members replace the
original base-first source-name slot; annotation detection uses that same
effective scope, so inherited explicit annotations prevent all-member inference.

C# `[SpecEvent(Ignore = true)]` excludes effective record fields/properties
before semantic mapping, getter validation, or projection. Annotation-presence
detection still runs on the full effective scope first, including ignored
annotations. Therefore a setup with only an ignored annotation has zero selected
semantic fields; it is not all-minus-ignored. Mixed setups must annotate desired
surviving members. An ignored child replaces and removes its base source-name
slot without falling back to the base declaration.

Capture decomposes property-setup arguments at operation entry, after object
initializers and any other caller-side mutation. Discovery and capture publish
the same folded surface. Rust replay reconstructs its own value with a struct
literal, decoding selected fields from the reference inputs. Participating
fields must be accessible to the generated invocation.

The property-setup opt-in is an author promise that omitted fields may safely
use `Default::default()`. Omission from recording alone is not evidence of
irrelevance and does not authorize reconstruction. The current implementation
requires the Rust type to implement `Default` for unannotated omissions;
explicitly ignored omissions may use the field-default fallback below. Failure
to provide a required default is a compile-time error. Zero-input
property setups may reconstruct an entirely default value. Types with
construction invariants must use an explicit producer instead.

Replay also accepts tuples of arity 2-4 and recursively supported collections,
maps, and optionals as operation and construction inputs through the
[checked storage bindings](registry-guided-collection-replay.md).
Primitive range, floating-point,
optional-tag, and collection semantics remain unchanged.

### Recursive decomposition (2026-10-28)

Property decomposition follows nested, explicitly opted-in record fields
recursively, including property-setup arguments to explicit setup producers.
Leaves appear in depth-first declaration order under their semantic field names.
Names remain flat: collisions anywhere in the folded operation are errors.
Resolve each nested field in its declaring type's native module, not the
operation's module. Cyclic property-setup declarations are rejected rather than
expanded indefinitely.

Capture snapshots the same leaves at operation or producer entry. Rust replay
reconstructs the nested record shape before invoking the operation or producer.
Empty nested property setups and omitted-field defaults retain the rules above.
An explicit producer still takes precedence over property decomposition of the
operation parameter it fills.

This recursive step does not descend into optional, list, set, or map elements.
Containers of supported primitive values remain supported; records inside
containers and undeclared record reconstruction remain outside this slice.
A null in a non-nullable nested property-setup member is reported like a null
in any non-nullable declared position: the test still runs, and capture drops
the scenario with a warning (see
[`null-values-are-capture-diagnostics.md`](null-values-are-capture-diagnostics.md)).
A nullable member (`T?`)
maps to `optional`, the same as Rust `Option<T>`, and is not decomposed.

### Ignored-field reconstruction (owner-approved 2026-11-17)

Rust ignored-field support uses this precedence:

1. Use captured semantic values for selected fields or setup-producer inputs.
   Defaults do not replace recorded values. An explicitly ignored field has no
   captured value, because it is absent from both the registry and trace.
2. If an explicit setup producer constructs the struct, use its result. The
   producer owns initialization of ignored fields; replay does not reset them
   or add a field-default requirement that the producer does not need.
3. Otherwise, within an explicitly opted-in property setup, use the whole
   struct's `Default` for omitted fields when available. Captured selected
   fields still override that default. If a custom struct default sets an
   ignored integer field to `42`, replay preserves `42`, not the integer
   default `0`.
4. If the struct has no `Default`, an ignored field's own `Default` is the
   last resort. Require `FieldType: Default` only when this fallback is needed,
   not when a producer or whole-struct default already supplies its value.

The last step permits field-only defaulting of ignored omissions; it does not
permit field-only defaulting of other, unannotated omissions. Such omissions
still need the whole-struct default or an explicit producer. Explicit producers
remain authoritative for the parameters they fill; the first step does not
bypass a producer to reconstruct its output from a recorded object.

This revises the earlier unconditional `Default` requirement for ignored fields
in [value decomposition](value-decomposition.md#annotations), and the
whole-struct-only restriction for property setups whose only omissions are
ignored fields. Ignored operation and producer parameters keep their existing
`Default` requirement.

This is implemented with private helpers local to the generated input decoder.
Non-overlapping reference receiver implementations select the whole-struct
default before falling back to a nested tuple of ignored-field defaults.
All selected inputs decode successfully before any default is invoked.
Property setups remain nongeneric named-field structs; no public trait or new
annotation is introduced.

Property-setup opt-in remains required. This does not enable general record
reconstruction, default a missing or malformed required reference input, or
define candidate-only input and inconclusive-replay policy.

## Boundaries

This supersedes the prohibition on defaulting omitted fields in
[`typed-async-client-replay.md`](typed-async-client-replay.md) only for explicit
property setups. `SpecEvent` projection alone never opts a type into input
reconstruction. General record reconstruction remains unsupported. No CTSC
document shape, CLI argument, comparison policy, runtime identity rule, or
golden-matrix meaning changes.

## Evidence

Native CTSC fixtures must cover constructor provenance, operation-entry property
projection, all-fields and subset reconstruction, renamed fields, folded-name
collisions, missing `Default`, and C#-reference-to-Rust replay of collection and
optional fields. Generated bundles must validate and paired registries must
agree.
