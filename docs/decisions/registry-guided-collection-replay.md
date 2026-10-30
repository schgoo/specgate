# Collection storage bindings in registry-driven replay

> **Status:** proposed; not implemented
> **Owner:** pending human approval
> **Date:** 2026-10-30

## Context

Replay is already registry-driven. It reads reference stimuli, links them to
candidate operations, and emits a candidate trace against the reference
registry. This design extends that path. It does not introduce another
registry, a second replay engine, or a new public mapping configuration.

The current linker requires exact normalized semantic type matches. Rust
`Vec<T>` declares a list, so it cannot currently satisfy a reference bag,
although it can store every bag item. Tuple inputs, ordered-map inputs, and
non-string map inputs also lack replay support. Inline tuple and record
outcomes cannot currently link to candidate declarations.

C# has two further projection gaps. `ILookup<K,V>` and `IGrouping<K,V>` expose
keys that the ordinary enumerable fallback does not preserve.

The existing CTSC 0.4.0 types are sufficient. This proposal changes producer
mappings and replay compatibility, not the wire format or Strict comparison.
The [collection-dimensions decision](collection-dimensions.md) remains the
accepted baseline. Its current replay boundaries remain in force until the
relevant changes below are approved and implemented.

## Proposed design

### Extend the existing linker

Keep the reference registry as the required semantic contract. Keep independent
candidate discovery unchanged: `Vec<T>` still declares an ordered, nonunique
collection, and native sets and maps keep their current defaults.

Replace exact type-string checks only where this proposal defines a checked
storage binding. Each binding relates a reference type to a candidate native
type and specifies supported input construction and captured-value conversion.
Use the existing planner and runner; do not add a separate binding service.
Internal type names and representation remain implementation choices.

Bindings belong to an operation and a value location, not to a Rust type
globally. One operation can use `Vec<i32>` for a list while another uses it for
a bag. The same rule applies to nested values.

Check item types, tuple positions, map keys and values, optional payloads, and
record fields through their own bindings. Retain primitive range checks,
operation identity, input names and order, completion channels, and declared
error names. Do not treat storage compatibility as permission to accept an
unrelated type.

Named types retain their existing identity requirements. Compare their
reachable declarations using the same nested binding rules, rather than
rejecting an otherwise permitted nested collection binding through an exact
JSON comparison. Imported outcome types remain outside this extension.

### Rust storage mappings

| Reference CTSC type | Accepted Rust storage |
|---|---|
| Ordered, nonunique collection (list) | `Vec<T>` |
| Unordered, nonunique collection (bag) | `Vec<T>` |
| Unordered, unique collection (set) | `Vec<T>` and existing supported `HashSet<T>` / `BTreeSet<T>` forms |
| Ordered, unique collection (ordered set) | `Vec<T>` |
| Unordered map | `Vec<(K,V)>` and existing supported string-keyed native maps |
| Ordered map | `Vec<(K,V)>` |
| Tuple | Matching Rust tuples of arity 2, 3, or 4, which already have native projection |

These are additions to existing supported bindings, not a removal of current
support. Every nested type must also have a supported binding. Borrowing and
setup rules remain unchanged.

A vector keeps bag multiplicity and can keep ordered-set positions. Unique
collections still require unique CTSC values; a vector does not remove that
requirement.

A vector of pairs keeps map associations and arbitrary entry order. The
reference type selects map encoding; the same native type used for a
collection of tuples remains a collection of tuples.

Use vectors of pairs for non-string map keys in this scope. General native
`HashMap<K,V>` / `BTreeMap<K,V>` projection is not required by this design.
A `BTreeMap` keeps key-sorted iteration, not arbitrary supplied entry order.
Do not bind it to an ordered map because one captured sample happens to be
sorted. Apply the same restriction to sorted sets and ordered collections.

### Preserve values during construction and conversion

Validate reference values before construction. Reject duplicate semantic
elements or keys where the registry requires uniqueness.

When construction inserts values into native sets or maps, detect any native
equality collision that would remove a distinct CTSC value or association.
Fail rather than silently overwrite or merge it. Check that the constructed
value still represents the reference input under the declared CTSC rules.

Convert actual candidate capture values through the checked bindings before
CTSC encoding. Apply the bindings to captured inputs, results, and declared
error payloads, including nested observed operations. Continue to invoke only
top-level reference operations.

For example, map pairs become entry records, while ordered sets remain arrays.
Use the existing trace encodings from [Trace section 8](../ctsc/trace.md#8-values).
Keep the reference registry bytes and digest unchanged.

Do not copy expected results, drop items, remove duplicates, sort significant
positions, merge map entries, or change completion channels to make a candidate
pass. Validate every converted candidate trace against the reference registry
before publication. Report the operation, value location, expected registry
type, and candidate storage type when a binding is unsupported.

### Separate input-order proposal

The current decoder sorts bags. Storage compatibility does not require that
behavior to change.

Separately, this design recommends preserving recorded array and map-entry
order during decoding and vector construction, including unordered values.
Native unordered sets and maps can still expose their native traversal order.
Canonical sorting belongs in comparison, not in preparation of vector inputs.

This changes replay stimulus selection. It needs explicit approval separate
from the storage mappings. Do not implement it as an incidental decoder fix.
CTSC still does not make the order of an unordered value significant.

### Preserve C# lookup and grouping keys

| C# declared contract | CTSC projection |
|---|---|
| `ILookup<K,V>` | Unordered map from `K` to an unordered, nonunique collection of `V` |
| `IGrouping<K,V>` | Inline record with `key: K` and `items: Bag<V>` |

`Bag<V>` is shorthand for the existing collection kind with `ordered: false`
and `unique: false`, not a new wire kind. The grouping record uses the existing
record kind from [Registry section 5.6](../ctsc/registry.md#56-record).

Resolve these contracts before the generic enumerable fallback. Include
concrete, inherited, and explicitly implemented interfaces. Discovery and
capture must use the same resolver and reject conflicting contracts.

For a lookup, obtain a fresh group enumerator. Read each group's key and
enumerate that group's values once with a fresh enumerator. For a grouping,
read its key and enumerate its values once. Do not advance caller-held cursors
or use consuming APIs. Ordinary enumeration can execute user code.

Keep duplicate items. Reject duplicate projected lookup keys under CTSC
equality, even if a native comparer considers the original keys distinct.
Preserve declared optional key types where available; do not convert keys to
strings or infer their type from observed contents.

### Bound inline record outcome matching

Permit an inline reference record outcome to bind to an annotated Rust struct
with exactly the same projected field names. Check every field type through
the binding rules above. Do not ignore extra fields or relax named-to-named
type identity. Apply the same check to nested inline record outcomes.

This permits a Rust grouping result with `key` and `items` fields without
requiring a fabricated C# type name. It does not add general record-input
construction. Grouping inputs remain unsupported unless the existing setup
mechanism already supplies a compatible folded input surface.

## Delivery slices

1. Add key-preserving C# lookup and grouping discovery and capture in the
   shared resolver, semantic mappers, and runtime projector.
2. Extend existing replay linking and runner conversion for collection
   storage, including nested bindings and vector-backed unique collections.
   Resolve the separate input-order proposal before changing decoder order.
3. Add tuple and map-entry input construction and captured-value conversion.
   Cover non-string keys through supported pair vectors.
4. Add inline-record outcome matching and a real C#-to-Rust grouping-result
   replay case.

Each slice updates its affected reference pages and limitations only when the
behavior is implemented. Use native CTSC tests and annotated fixtures. Add
matrix rows for new fixture components and regenerate goldens through the
existing harness; do not weaken parity, linkage, or coverage checks.

## Validation requirements

- Bag `[a,b,a]` equals `[a,a,b]`, but not `[a,b]`; construction and conversion
  retain all occurrences.
- A vector-backed set or ordered-set result with duplicate semantic items
  fails validation; it is never deduplicated.
- Reversing an ordered set or ordered map changes the comparison result.
- If the input-order proposal is approved, replay passes bag items to a
  vector in their recorded order, including duplicates.
- Lookup and grouping key changes affect comparison. Duplicate lookup keys
  after projection fail rather than merge.
- Priority-queue pairs retain both element and priority through bag and tuple
  bindings.
- Ordered and non-string maps use the required entry-record encoding. Pair
  vectors retain associations; native insertion cannot silently collapse keys.
- Nested collection fields use reference dimensions without weakening named
  identity or accepting unrelated field types.
- Existing list, native set, and unordered string-keyed map cases still pass.
- Unsupported tuple arities, grouping inputs, and incompatible structures
  fail during planning with actionable diagnostics.

Run focused tests for each implemented slice, regenerate affected goldens,
and run `just check` before implementation handoff.

## Outside this design

Custom collection declarations and derive support remain in
[#116](https://github.com/schgoo/specgate/issues/116). This proposal does not
add general record construction, arbitrary native map keys, erased
collections, cursor lifecycles, new tuple arities, or C# candidate replay.
It does not resolve generic specialization ownership or map C# nulls and
exceptions to Rust `Option` and `Result` completion channels.
