# Custom Rust data structures: serialization extension

**Add lossless native collection/map values, then a `spec_collection` attribute macro.
Reuse the existing type, serialization, and construction contracts.**

> **Status:** attribute macro selected; detailed API proposed; not implemented
> **Reviewer:** @schgoo
> **Date:** 2026-11-10
> **Issue:** [#116](https://github.com/schgoo/specgate/issues/116)

## What changes

Authors tell SpecGate how their structure maps to an existing Conformance Trace
Semantic Conventions (CTSC) type. For example, a vector-backed bag declares an
unordered, nonunique collection and serializes every item, including duplicates.
SpecGate does not need a built-in mapping for each native container.

| Responsibility | Use the existing contract |
|---|---|
| Type declaration | Existing type metadata; `SpecEvent` for records/unions and `spec_collection` for collections/maps |
| Serialization | `ToNativeValue`; generated or hand-written |
| Reconstruction | Existing `SpecEvent` reconstruction and `SpecSetup` mechanisms |

No new `SpecType` trait, `FromNativeValue`, or `ReplayConstructor`.
`spec_collection` is a separate authoring macro, not a separate type contract.
Serialization does not grant permission to construct a value.
The existing `SpecEvent` derive and trait remain unchanged in this slice.
Renaming that API or unifying the authoring macros is a separate decision.

## Prerequisite: lossless native values

The current `Value::Set(BTreeSet<Value>)` can discard projected duplicates.
`Value::Map(BTreeMap<String, Value>)` cannot retain arbitrary keys, entry order,
or colliding keys. Neither is sufficient as the general serialization target.

Proposed additions to the existing native `Value` enum:

| Variant sketch | Payload |
|---|---|
| `Collection(Vec<Value>)` | Items in traversal order, retaining every occurrence |
| `MapEntries(Vec<(Value, Value)>)` | Key/value associations in traversal order, retaining every entry |

The names remain proposed. `ordered` and `unique` stay in the registry rather
than becoming a second, possibly conflicting declaration on each value.
The same collection payload supports lists, bags, sets, and ordered sets.
Map keys remain unique under CTSC equality, checked before publication.

Route collection/map projection through these lossless forms. Do not construct
a native set or dictionary first and lose evidence of collisions. Existing
record, tuple, optional, and outcome representations are not redesigned here.

Encoding uses the declared registry type: collections become arrays; unordered
string-keyed maps become key/value lists; other maps become arrays of entry
records. The serializer supplies entries without assembling wire-format records.
Update native sidecar serialization, decoding, and replay conversion together.
The CTSC wire format and reference registry remain unchanged.

## Violation policy and sequencing

Detect declared-shape and uniqueness violations without silently discarding
data. Until record-and-warn is implemented, preserve today's behavior and
reject them before publication. This extension can therefore ship first.
Future violation handling and evidence encoding follow
[#126](https://github.com/schgoo/specgate/pull/126), not a separate policy here.
This slice does not freeze future value or replay semantics.

Distinguish observed contract violations from known serializer shape bugs.
A duplicate element or key does not by itself prove a product bug: custom
projection can also introduce collisions. Attribute a wrong shape to the
serializer only when that origin is established; canonical codes belong to #126.

CTSC requires unique keys in `kvlistValue`, so an unordered string-keyed map
with duplicate keys cannot be published in its ordinary encoding. Retain every
conflicting entry in `MapEntries` until rejection. Future record-and-warn must
define both lossless violation evidence and what occupies the ordinary
input/result slot; never silently choose one conflicting value. That encoding
decision belongs to #126.

## Proposed author syntax

Use one attribute containing the declaration, without a companion derive:

```rust
#[specgate::spec_collection(items = T, ordered = false, unique = false)]
pub struct Bag<T> {
    items: Vec<T>,
}
```

The attribute macro publishes the collection declaration through existing metadata.
Its default `ToNativeValue` implementation uses `IntoIterator` on `&Self`, not
on owned `Self`. This example must expose borrowed iteration or use a manual
serializer; the macro does not guess which field holds the items.
The wire value is the collection itself, not a record with an `items` field.

A derive could generate the same implementations but would need a separate
configuration attribute. The attribute macro keeps configuration in one place.
It must preserve the original type declaration, including fields, visibility,
generics, and other attributes; it only adds generated implementations and metadata.

| Shape | Declaration |
|---|---|
| Collection | `#[specgate::spec_collection(items = T, ordered = ..., unique = ...)]` |
| Map | `#[specgate::spec_collection(keys = K, values = V, ordered = ...)]` |

Map keys are always unique. `ordered` means position matters, not sortedness.
Type expressions can contain generics and nested supported types.

Map iteration accepts `(&K, &V)` or `&(K, V)`, normalized internally without a
new public entry trait. Borrowed `IntoIterator` alone is not proof of safe
capture: channel receivers, for example, consume messages during iteration.

**Manual serialization:** `#[specgate::spec_collection(..., manual)]` leaves
`ToNativeValue` to the author; the macro still emits metadata and the existing
type marker. It does not emit a conflicting serialization implementation.
An optional `serialize_with = Self::project` shorthand could delegate to a
function taking `&Self` and returning `Value`; whether to include it is open.
These modes replace default iteration and require no reconstruction function.

## Rules

- **Declare, do not infer.** Collection/map semantics come from the annotation,
  never the backing field, type name, sort order, or observed contents.
  Existing record/union annotations and standard mappings keep their defaults.
- **Preserve the value.** Retain every occurrence and map entry, including
  duplicates that violate uniqueness. Detect duplicate semantic elements or
  keys; never silently merge them. Reject violations under the current policy.
- **Check the declared shape.** A custom serializer must produce values that
  match its declaration, including nested types. Reuse existing encoding and
  linked validation for inputs, results, and errors before publication, with
  violation handling governed by the sequencing above.
- **Do not consume caller state.** Generated projection uses fresh borrowed
  traversal, not draining, dequeuing, or advancing an existing cursor.
  Custom serialization code remains the author's responsibility.
- **Do not add construction behavior.** Keep existing setup selection,
  reconstruction opt-ins, replay storage bindings, and bag input sorting.
  A serializable type without a supported construction path remains valid for
  capture; unsupported replay inputs retain the existing limitation behavior.

## Integration

| Area | Work required |
|---|---|
| Runtime values | Add lossless collection and map-entry payloads. Update sidecar and CTSC encoding/decoding without changing CTSC wire shapes. |
| `spec_collection` attribute macro | Generate collection/map metadata and borrowed iteration or manual projection. Preserve the original declaration and reuse existing traits; reject combining it with the record/union derive on one type. |
| Runtime metadata | Extend the existing metadata path to carry custom semantic shapes. Keep `SpecEvent` record/union defaults and `ToNativeValue` unchanged in role. |
| Discovery | Resolve concrete generic arguments and nested shapes without matching custom types by bare name alone. Reject endlessly expanding inline declarations. |
| Capture/replay | Use the same declarations and native values through existing encoding, validation, conversion, and setup paths. Do not introduce a constructor registry. |

**Delivery order:** first the native value migration with existing producers,
then the custom collection macro. Each slice uses the existing validation gate.
Neither adds arbitrary record construction, C# adapters, new CTSC wire kinds,
or comparison rules. Existing named record identity remains unchanged.

## Required evidence

- Capture a generic vector-backed bag through a real annotated operation.
  Reordering compares equal; changing duplicate counts does not.
- Cover nested shapes, concrete generic specializations, and same-named types
  in different modules.
- Verify native values retain order, multiplicity, non-string keys, and
  colliding projected keys until validation. Cover nested values and each map
  wire encoding.
- Exercise borrowed iteration and manual serialization. Reject incompatible
  values and duplicate unique elements/keys under today's policy, retaining
  evidence until rejection. Reject conflicting shape annotations.
- Verify the attribute preserves the original type declaration and works with
  unrelated derives and attributes in either order. Reject combining it with
  `#[derive(SpecEvent)]` regardless of attribute order.
- Exercise existing setup-based replay without new construction APIs.
  Verify serialization alone does not enable unsupported reconstruction.
- Extend the native golden matrix, regenerate artifacts, update mapped docs,
  and run `just check`.

**For review:** native variant names and migration, collection/map attribute
syntax, and whether a callback shorthand is useful beyond manual serialization.
The earlier proposal's parallel type and construction APIs are removed.

Related: [CTSC type rules](../ctsc/registry.md#5-type-system),
[capture non-interference](../ctsc/trace.md#32-non-interference),
[existing reconstruction opt-ins](../decisions/constructor-and-property-setups.md),
and [accepted replay storage bindings](../decisions/registry-guided-collection-replay.md).
