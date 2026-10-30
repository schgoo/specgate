# Collection shape and semantic dimensions

## Decision

CTSC 0.4.0 intentionally breaks the 0.3.0 collection wire format. Homogeneous
element collections use `kind: collection` with required `ordered` and `unique`
booleans. Key/value associations retain `kind: map`, inherently unique keys, and
a required `ordered` boolean. Strict comparison is versioned as
`ctsc.strict/0.2.0`.

The owner approved separating structure from dimensions and proceeding with a
breaking CTSC change. There is no old-wire compatibility reader or default for
an omitted dimension. Unsupported semantic fields are rejected, not ignored.

## Rationale

List, bag, set, and ordered set differ in equality and validity, not in item
relationships. Queue/stack implementation details do not need new wire kinds:
their snapshots are ordered collections, while operation traces expose their
behavior. Future dimensions can extend the model explicitly without multiplying
structural kinds. This does not authorize ignoring future fields.

`ordered` means position matters, not sortedness. A bag retains duplicate
counts. Uniqueness uses recursive CTSC equality, so two native objects or keys
that project to one semantic value cannot be silently merged.

Ordered maps always use arrays of key/value entry records, even with string
keys. Unordered string-keyed maps retain OTLP key/value-list encoding.

## Producer boundary

C# discovery and capture share interface-based collection resolution. Generic
enumerable contracts default to bags; list and ordered-enumerable contracts,
and queue/stack/linked-list families, declare order. Set contracts declare
uniqueness. Dictionary contracts take precedence over their enumerable/list
views. Concrete, inherited, and explicitly implemented generic contracts work
without requiring user-authored adapters.

Existing sorted-set and sorted-map defaults remain unordered in both C# and
Rust: sorting an implementation's traversal is not a new promise that position
is part of the portable API contract. Generic ordered dictionaries declare
ordered maps. Priority queues expose bags of element/priority tuples through
`UnorderedItems`; capture never dequeues.

Ordinary collection enumeration may execute user code. The producer obtains a
fresh enumerator; it does not advance a caller-owned cursor or choose a
consuming API. No new lifecycle model is introduced.

## Boundaries

Non-generic erased collections, asynchronous streams, caller-held enumerators,
buffers without enumerable contracts, configurable collection-semantic
overrides, and automatic cross-language shape/outcome conversion are not added.
In particular, Rust `Vec<T>` still declares an ordered, nonunique collection:
it is not silently compatible with a C# bag. Ordered-map input replay remains
unsupported rather than discarding entry order. Generic specialization ownership
and C# null/exception versus Rust `Option`/`Result` mapping remain separate work.

Existing bundles must be recaptured; checked-in generated bundles are
regenerated through the golden harness.

## Proposed extension

[Collection storage bindings in registry-driven replay](registry-guided-collection-replay.md)
proposes checked Rust storage mappings and key-preserving C# lookup/grouping
projections. Replay is already registry-driven; the proposal extends its
existing linker and value conversion. It does not change the CTSC 0.4.0 wire
format. The replay input-order recommendation is a separate proposed change.
These proposals are not implemented and do not yet replace the boundaries
above.
