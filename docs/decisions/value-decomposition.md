# Language-neutral value decomposition

> **Status:** proposed
> **Owner:** @schgoo
> **Date:** 2026-11-04
> **Tracking:** #125, #127

## Context

SpecGate does not require Rust and C# implementations to be the same. It
**represents each side faithfully and reports where they differ**. Today it
falls short of that goal:

- Discovery copies native wrapper nesting into the registry. `Option<T>`,
  `T?`, and `Result<Option<T>, E>` give registries that differ in shape, not
  in meaning.
- Rust `Option<Option<T>>` and `Option<()>` have no consistent encoding.
- A C# `null` from a nullable-oblivious reference is a capture fault, so the
  scenario is dropped. Example: the DSAPI SDK returns `null` for an HTTP 404.
- A scenario that does not match its registry is dropped, so the difference is
  never reported.
- C# record types omit inherited members (#127).

## Decision

### Goal

Represent everything each language can do, in one neutral model. Report
differences; do not hide them. Optional annotations can map names so that the
same concept compares equal.

### Concepts

| Concept | Meaning |
|---|---|
| scalar | A leaf value: bool, integer, float, string, bytes, or unit. |
| record | Named fields. All fields are present. |
| one_of | Named variants. Exactly one variant is present. |
| collection | Items with laws: ordered, unique, or keyed. |

Vocabulary follows Rust: `optional`, `none`, and `some`.

### Decomposition into paths

- A **record** decomposes into paths: `x.a`, `x.b.c`.
- A **one_of** uses the variant tag as a path segment: `x.circle.r`, or
  `x.empty: ()` for a unit variant. Exactly one variant path exists.
- A **collection** stops decomposition and keeps its laws. Each item decomposes
  again, relative to the item.
- The **type** defines the set of paths. A value never moves its path.

### Slot properties

Any slot (field, input, result, item, map value, or variant payload) can have:

| Property | Meaning |
|---|---|
| `type` | The concept or scalar type. |
| `optional` | The source declares that the value can be `none`. |
| `oblivious` | The source does not declare whether the value can be `none`. |

`none` is a distinct value. It is not unit and it is not absence:

| `Option<()>` | Path value |
|---|---|
| `None` | `x: none` |
| `Some(())` | `x: ()` |

`none` at a record slot is different from `none` in its fields:
`x: none` and `x.a: none` are different paths.

### Nested `Option`

Each nested `Option` level adds one `some` segment. The innermost `Some` is
elided. Every state has a distinct path, so nothing is lost.

| `Option<Option<i32>>` | Path value |
|---|---|
| `None` | `x: none` |
| `Some(None)` | `x.some: none` |
| `Some(Some(5))` | `x.some: 5` |

C# `T?` cannot nest. C# can nest `SpecGate.Annotations.Option<T>`, which uses
the same encoding.

### Language mapping

| Native shape | Concept |
|---|---|
| Rust `Option<T>`; C# `T?`, `Nullable<T>`, `SpecGate.Annotations.Option<T>` | `optional` slot |
| C# reference type without a nullable context | `oblivious` slot |
| C# `null` in an `optional` or `oblivious` slot | `none` |
| Rust `enum` with data; C# abstract type with concrete derived types | one_of |
| C# concrete class, including a concrete base class | record |
| Rust `Vec<T>`, C# `List<T>`, and similar | ordered collection |

- A C# record type includes inherited public members, base-first (#127).
- A concrete base class stays a record, even if it acts like an enum. No
  reliable signal identifies such classes.

### Annotations

Use the existing annotations. Do not add new ones.

- **Rename:** `[SpecInput("name")]` / `#[spec_input("name")]`, and SpecEvent
  `name`.
- **Ignore:** add `Ignore = true` / `ignore` to `SpecInput`/`spec_input` and to
  SpecEvent fields. Ignored inputs are not captured or compared.
- **Error names:** `SpecException` and the Rust error annotation can give a
  semantic error name, so that a C# exception type and a Rust error variant or
  kind compare by name.

### Outcomes

- Rename the outcome property `empty` to `optional`.
- Remove the rule that `empty` requires `result` (`registry.md` §4.1). No
  `result` means unit, so `Option<()>` is `optional: true` with no `result`.
- Normalize Rust nesting: `Result<Option<T>, E>` gives `result: T`,
  `optional: true`, and `errors`.

### Record and warn

Capture and validation never drop a scenario because it does not match its
registry. They record the observed behavior and report a **registry
violation**. Discovery does not widen the registry.

### Replay inputs

| Case | Action |
|---|---|
| Input only on the reference side | Skip the input. Add a warning to the result. |
| Input only on the replay side | Use the language or type default, with a warning. Else `none` if `optional`. Else skip the scenario, with a warning. |

Generated values (property testing) are deferred.

### Comparison report

| Class | Source |
|---|---|
| Signature difference | Registry vs registry. `oblivious` vs any optionality is not a difference. |
| Behavior difference | Trace vs trace. |
| Registry violation | Trace vs its own registry. |

## Open questions

1. OTLP encoding of `none`. The empty `AnyValue` is invalid today
   (`trace.md` §8) and OTLP tools can drop it. Prefer a reserved typed marker.
2. Storage: proposed to store the trace as a nested tree and decompose it in
   the validator and comparator.
3. A variant with one payload field: proposed to put the payload directly at
   the variant path, so Rust `Set(T)` and C# `Set(T Value)` both give `x.set`.
4. Variant and field name casing across languages.
5. Whether the `conformance.empty` event is renamed with the property.
6. The trace form of a registry violation, so that linked validation still
   passes.
7. Error annotation syntax, including a Rust error with a string `kind` field.
8. `Option<Result<T, E>>`: keep its mapping or reject it.

## Consequences

- Normative changes to CTSC `registry.md` §4.1 and §5, `trace.md` §8, and
  `comparison.md`.
- The `optional<T>` registry kind is replaced by the `optional` slot property.
- Rust and C# discovery, capture, replay, validators, comparator, and goldens
  change. The hand-authored parity entries in `test/behavior/matrix.json`
  change; that is an owner decision. CTSC consumers need a migration note.
- Strict golden capture keeps failing on a registry violation.
- Implement in slices, for example: outcomes; `none` encoding and nested
  `Option`; `oblivious`; inherited members (#127); `ignore` and replay inputs;
  record and warn; slot properties.

## Validation

Focused registry, trace, capture, replay, validator, and comparator tests, plus
regenerated goldens with cross-language parity rows for `Option<T>` / `T?`,
`Option<Option<T>>`, `Option<()>`, `Result<Option<T>, E>`, oblivious
references, and inherited members.

## Supersedes / superseded by

Will supersede
[`null-values-are-capture-diagnostics.md`](null-values-are-capture-diagnostics.md)
when accepted.