# Custom Rust data structures: serialization extension

**Add lossless native collection/map values, then a `spec_collection` attribute macro.
Extend `SpecEvent` for compiler-directed type metadata; reuse serialization and
construction contracts.**

> **Status:** metadata direction accepted; detailed API proposed; not implemented
> **Reviewer:** @schgoo
> **Date:** 2026-11-11
> **Issue:** [#116](https://github.com/schgoo/specgate/issues/116)

## What changes

Authors tell SpecGate how their structure maps to an existing Conformance Trace
Semantic Conventions (CTSC) type. For example, a vector-backed bag declares an
unordered, nonunique collection and serializes every item, including duplicates.
SpecGate does not need a built-in mapping for each native container.

| Responsibility | Use the existing contract |
|---|---|
| Type declaration | Extend existing `SpecEvent` with type metadata; record/union derives and `spec_collection` supply implementations |
| Serialization | `ToNativeValue`; generated or hand-written |
| Reconstruction | Existing `SpecEvent` reconstruction and `SpecSetup` mechanisms |

No new `SpecType` trait, `FromNativeValue`, or `ReplayConstructor`.
`spec_collection` is a separate authoring macro, not a separate type contract.
Serialization does not grant permission to construct a value.
The existing `SpecEvent` trait gains a static metadata method. The record/union
derive emits that method as well as its existing implementations;
`spec_collection` emits it for collections/maps. Renaming `SpecEvent` or
unifying the authoring macros remains a separate decision.
See the [accepted metadata direction](../decisions/custom-collection-metadata.md).

## Compiler-directed metadata

The collection attribute emits a generic metadata implementation independently
of serialization. The operation macro attaches a compiler-instantiated callback
to its existing link-time operation metadata. Discovery executes that callback
for the operation's concrete inputs and output, including declared generic
specializations. Rust resolves aliases, module scope, and generic substitution;
SpecGate does not guess identities from bare names, parse `type_name` strings,
or require authors to register every collection specialization.

An isolated experiment demonstrated this route with an experimental method:

```rust
fn __specgate_type_metadata() -> Option<String>
where
    Self: Sized;
```

The method name and `Option<String>` representation are not the final API.
Production metadata must retain named-type ownership, not merely an unqualified
name in a string. `Self: Sized` keeps existing `dyn SpecEvent` usage possible.
A default unavailable result allows existing empty marker implementations to
compile without inventing metadata; discovery must reject unavailable required
metadata with a useful diagnostic. This does not promise full source
compatibility: new standard-type and blanket reference implementations need
coherence review against downstream implementations before release.

The change broadens `SpecEvent` beyond a record/union marker. Add metadata for
supported standard types through this same path. Do not add a discovery-only
`'static` requirement to borrowed values, change the authored generic operation's
bounds merely for discovery, or invoke a serializer to discover a type.
Existing generic-operation specialization restrictions remain separate.

Named records remain named records. For `Bag<Order>`, the collection appears
inline in the operation's result type and references the component's `Order`
definition; it does not introduce a nominal `Bag` record. A record owned by a
different component must retain that owner. Cross-document references also
require an import with exact version/digest and a component dependency, as
specified by [registry resolution](../ctsc/registry.md#8-named-type-resolution).
Do not silently rebind an imported `Order` to a same-named local record.

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
Map keys remain unique under CTSC equality, checked by the narrow annotation
checks described below.

Route collection/map projection through these lossless forms. Do not construct
a native set or dictionary first and lose evidence of collisions. Existing
record, tuple, optional, and outcome representations are not redesigned here.

Encoding uses the declared registry type: collections become arrays; unordered
string-keyed maps become key/value lists; other maps become arrays of entry
records. The serializer supplies entries without assembling wire-format records.
Update native sidecar serialization, decoding, and replay conversion together.
The CTSC wire format and reference registry remain unchanged.

**Migration decision:** a breaking cleanup of obsolete native collection
variants is allowed. Cut affected writers and readers over together; old native
sidecars can be regenerated, so no legacy sidecar reader is required. Keep
representations still needed for records, tuples, optionals, and outcomes.

## Violation policy and sequencing

The following are permanently capture-stopping **annotation errors**:

- duplicate semantic keys in a declared map;
- duplicate semantic elements in a collection declared `unique: true`;
- a custom serializer returning a shape incompatible with its declaration,
  including nested shapes.

Reject these regardless of whether the native product value or its projection
introduced the conflict. Diagnostics identify the value path and offending
key/element or expected and observed shapes. Never deduplicate silently.
Repeated bag elements remain legitimate and retain their multiplicity.
These errors are not deferred product-behavior warnings.

Align with [#126](https://github.com/schgoo/specgate/pull/126): capture and replay
run structural trace validation, not a general linked-validation walk against
the registry. Structural failure is a SpecGate defect. Only `compare` and
`validate linked` perform general product-value checking; golden validation
remains linked and strict. Narrow annotation checks are the explicit
capture-stopping exception and may reuse recursive typed equality without
calling the full linked validator. This is the intended integration policy,
not a claim that #126's changes are already implemented on this branch.

CTSC requires unique keys in `kvlistValue`, so an unordered string-keyed map
with duplicate keys cannot be published in its ordinary encoding. Retain every
conflicting entry in `MapEntries` until rejection and diagnostic generation.
Because capture stops, no warning-event encoding or replacement ordinary
input/result value is needed for these annotation errors. Never silently choose
one conflicting value.

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
`ToNativeValue` to the author; the macro still emits the metadata-bearing
`SpecEvent` implementation. It does not emit a conflicting serialization implementation.
An optional `serialize_with = Self::project` shorthand could delegate to a
function taking `&Self` and returning `Value`; whether to include it is open.
These modes replace default iteration and require no reconstruction function.

## Rules

- **Declare, do not infer.** Collection/map semantics come from the annotation,
  never the backing field, type name, sort order, or observed contents.
  Existing record/union annotations and standard mappings keep their defaults.
- **Preserve the value.** Retain every occurrence and map entry, including
  duplicates that violate uniqueness. Detect duplicate semantic elements or
  keys; never silently merge them. Permanently reject these annotation errors.
- **Check the declared shape.** A custom serializer must produce values that
  match its declaration, including nested types. Check that serialization
  contract narrowly for inputs, results, and errors; do not add general linked
  validation during capture/replay.
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
| Runtime metadata | Add the static metadata method to `SpecEvent`, update the record/union derive, and implement supported standard types. Review defaults, object safety, and downstream coherence; keep `ToNativeValue` focused on serialization. |
| Discovery | Consume compiler-instantiated operation callbacks. Preserve named-type component/registry ownership through normalization and import resolution. Cover record-field type use sites too; reject unavailable metadata and endlessly expanding inline declarations. |
| Capture/replay | Use lossless values and narrow annotation checks through existing encoding, conversion, and setup paths. Run structural trace validation, not general linked validation. Do not introduce a constructor registry. |

**Delivery order:** first the native value migration with existing producers,
then metadata-method/callback integration and the custom collection macro.
Each slice uses the existing validation gate.
Neither adds arbitrary record construction, C# adapters, new CTSC wire kinds,
or comparison rules. Existing named record identity remains unchanged.

## Generic metadata feasibility probe

The executable probe is
[`collection_metadata_proof.rs`](../../rust/crates/specgate-ctsc/tests/collection_metadata_proof.rs).
It registers declarations by hand, obtains concrete native spellings from
compiler-checked Rust function signatures and types, substitutes generic
arguments, and passes the resulting semantic types through the existing CTSC
registry encoder and Registry validator.

The cases cover two instantiations of one generic bag; same-named containers
in different modules; imports and type aliases; nested collection and map
arguments; declaration templates that apply another generic constructor;
named record items; and rejection of missing, duplicate, or malformed
declarations. Both directly growing and indirect expansion cycles are
rejected, while finite nesting such as `Bag<Bag<Order>>` is accepted.
Containers become structural CTSC collections/maps, not named record wrappers.

**Boundary:** this proves substitution and registry representability given
correct native identities. It does not prove automatic discovery or metadata
emission. Today's Rust mapper discards module paths and unknown generic
arguments; the implementation must carry compiler-resolved identities through
the existing metadata path, including use sites and declaration templates.
The probe uses `type_name` spellings for its fixtures, not a new identity
contract: those diagnostic strings alone are not guaranteed globally unique.
Do not resolve imports by guessing a declaration from its bare name.
The probe adds no public trait, macro, capture behavior, or reconstruction path.

The cross-component cases manually provide a qualified record identity, encode
it through the existing encoder, and validate both a same-document dependency
and an imported registry. The import case moves the encoded model component
into a separate document, explicitly adds the import/dependency/reference
qualifiers, and verifies exact-byte digest checking. A same-named local record
must not capture the reference. Missing imports/dependencies and tampered
import bytes reject. This proves existing registry support, not automatic
generation of those qualifiers from Rust.

## Compiler-dispatch experiment and remaining boundaries

A separate isolated experiment changed the actual runtime `SpecEvent`, the
record derive, the collection attribute, and operation macro expansion.
Real link-time `OpMeta` entries carried concrete metadata callbacks. Six
positive tests covered aliases and scoped same-named types, nested generics,
borrowed non-`'static` values, independent manual serialization, unavailable
legacy metadata, object safety, and real registry encoding/validation.
The encoded registry retained the genuine `Order` and renamed `Invoice`
records without collection wrapper records.

Focused tests, a reconstruction compile-fail doctest, Clippy, formatting, and
diagnostic-checked compiler probes passed. The probes observed `E0599` when
calling this method on the unchanged API, `E0275` for an expanding declaration,
and `E0277` for reconstruction without permission. The first is evidence that
this particular hook is absent today, not proof that every unchanged-trait
alternative is impossible. Trait-solver overflow is fail-closed evidence,
not the desired final SpecGate annotation diagnostic.

That experiment remains outside this design PR; its source is not installed
or reproducible from this checkout. It is not production discovery: a test
bridge transferred callbacks into the semantic schema before real encoding.
The checked-in resolver probe above remains manually registered and is
reproducible here. Do not confuse the two evidence boundaries.

Production callback consumption, compiler-directed record-field metadata,
complete component/registry qualification, the full standard-type surface,
lossless native-value migration, annotation checks, setup folding, and candidate
replay integration remain implementation work. No end-to-end custom collection
trace was captured or compared by either metadata proof. Registry shape alone
does not establish capture/replay correctness.

## Required evidence

- Capture a generic vector-backed bag through a real annotated operation.
  Reordering compares equal; changing duplicate counts does not.
- Cover nested shapes, concrete generic specializations, and same-named types
  in different modules. Verify borrowed values, manual serializers, metadata
  availability, object safety, and standard/reference implementation coherence.
- Automatically discover a collection of another component's named record,
  then exercise imported-registry qualification. Preserve explicit ownership
  even with same-named local records; reject missing imports/dependencies and
  incorrect versions/digests. The hand-qualified registry proof is not enough.
- Verify native values retain order, multiplicity, non-string keys, and
  colliding projected keys until validation. Cover nested values and each map
  wire encoding.
- Exercise borrowed iteration and manual serialization. Reject incompatible
  serializer shapes and duplicate unique elements/keys as permanent annotation
  errors, retaining evidence until rejection. Verify diagnostics name the path
  and offending value. Reject conflicting shape annotations. Test annotation
  checks separately from general linked validation.
- Verify the attribute preserves the original type declaration and works with
  unrelated derives and attributes in either order. Reject combining it with
  `#[derive(SpecEvent)]` regardless of attribute order.
- Exercise existing setup-based replay without new construction APIs.
  Verify serialization alone does not enable unsupported reconstruction.
- Extend the native golden matrix, regenerate artifacts, update mapped docs,
  and run `just check`.

**For review:** native variant names, the production metadata method's name and
typed representation, collection/map attribute syntax, and whether a callback
shorthand is useful beyond manual serialization. The coordinated breaking
sidecar cutover and existing-trait metadata direction are accepted.
The earlier proposal's parallel type and construction APIs are removed.

Related: [CTSC type rules](../ctsc/registry.md#5-type-system),
[capture non-interference](../ctsc/trace.md#32-non-interference),
[existing reconstruction opt-ins](../decisions/constructor-and-property-setups.md),
and [accepted replay storage bindings](../decisions/registry-guided-collection-replay.md).
