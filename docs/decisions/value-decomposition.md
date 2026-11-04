# Language-neutral value decomposition

> **Status:** proposed
> **Owner:** @schgoo
> **Date:** 2026-11-04
> **Tracking:** #125

## Context

Rust and C# implement the same concept in different shapes. One example is
`Option<T>` vs `T?`. Another is `Result<Option<T>, E>` vs a nullable return
with exceptions. Today discovery copies the native wrapper nesting into the
registry, so equivalent APIs produce registries that do not match. Rust
`Option<Option<T>>` and `Option<()>` have no consistent encoding.

The registry and trace must describe **concepts**. Each language maps its
native shapes onto those concepts.

## Decision

### Concepts

| Concept | Meaning |
|---|---|
| scalar | A leaf value: bool, integer, float, string, bytes, or unit. |
| record | Named fields. All fields are present. |
| one_of | Named variants. Exactly one variant is present. |
| collection | Items with laws: ordered, unique, or keyed. |

Vocabulary follows Rust: `optional`, `none`, and `some`.

### Decomposition into paths

- A **record** decomposes into leaf paths: `x.a`, `x.b.c`.
- A **one_of** uses the variant tag as a path segment: `x.circle.r`, or
  `x.empty: ()` for a unit variant. Exactly one variant path exists.
- A **collection** stops decomposition and keeps its laws. Each item decomposes
  again, relative to the item.
- The **type** defines the set of leaf paths. A value never moves its path.

### Slot properties

Each leaf slot has two properties:

- `type`: the scalar or collection type.
- `optional`: the value can be `none`.

`none` is a distinct value. It is not unit and it is not absence:

| `Option<()>` | Path value |
|---|---|
| `None` | `x: none` |
| `Some(())` | `x: ()` |

### Nested `Option`

Each nested `Option` level adds one `some` segment. The innermost `Some` is
elided. Every state has a distinct path, so nothing is lost.

| `Option<Option<i32>>` | Path value |
|---|---|
| `None` | `x: none` |
| `Some(None)` | `x.some: none` |
| `Some(Some(5))` | `x.some: 5` |

C# cannot nest `T?`, so a C# `T?` always uses the path `x` only.

### Language mapping

| Native shape | Concept |
|---|---|
| Rust `Option<T>`, C# `T?` / `Nullable<T>` | `optional` slot |
| C# `null` in a `T?` position | `none` |
| Rust `enum` with data, C# abstract type with concrete derived types | one_of |
| Rust `Vec<T>`, C# `List<T>` and similar | ordered collection |

User types map onto these concepts through annotations. Annotations do not
add new concepts.

### Outcomes

- Rename the outcome property `empty` to `optional`.
- Remove the rule that `empty` requires `result` (`registry.md` §4.1). No
  `result` means unit, so `Option<()>` is `optional: true` with no `result`.
- Normalize Rust nesting: `Result<Option<T>, E>` gives `result: T`,
  `optional: true`, and `errors`.

### Registry mismatches

Capture and validation record observed behavior that does not match the
registry and emit a warning. They do not drop the scenario. Discovery does not
widen the registry automatically.

## Open questions

1. The OTLP encoding of `none` in the stored trace. Proposed: the empty
   `AnyValue` `{}`, because `{"None": {}}` conflicts with a user variant named
   `None`.
2. A record field whose value is `none`: proposed to stay present with value
   `none`. A missing field stays invalid.
3. Storage: proposed to store the trace as a nested tree and decompose it in
   the validator and comparator.
4. A variant with one payload field: proposed to put the payload directly at
   the variant path, so Rust `Set(T)` and C# `Set(T Value)` both give `x.set`.
5. Annotation surface for mapping user types onto concepts, for example a
   presence flag or a variant that maps to `none`.
6. Scope of record and warn: outcomes only, or every registry mismatch.
7. `Option<Result<T, E>>`: keep its mapping or reject it.

## Consequences

- Normative changes to CTSC `registry.md` §4.1 and §5 and `trace.md` §8.
- The wrapper value kind `optional<T>` is replaced by the `optional` slot
  property.
- Rust and C# discovery, capture, replay, validators, and goldens change.
  CTSC consumers need a migration note.

## Validation

Focused registry, trace, capture, replay, and validator tests, plus regenerated
goldens with cross-language parity rows for `Option<T>` / `T?`,
`Option<Option<T>>`, `Option<()>`, and `Result<Option<T>, E>`.

## Supersedes / superseded by

Will supersede
[`null-values-are-capture-diagnostics.md`](null-values-are-capture-diagnostics.md)
when accepted.
