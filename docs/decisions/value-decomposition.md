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
- Every concrete C# subclass of an abstract `[SpecEvent]` class becomes a
  union variant, even if the hierarchy is open.

## Decision

### Goal

Represent everything each language can do, in one neutral model. Report
differences; do not hide them. Optional annotations can map names so that the
same concept compares equal.

### Concepts

| Concept | Meaning |
|---|---|
| scalar | A leaf value: bool, integer, float, string, bytes, or unit. |
| record | Named fields. All fields are present. A tuple is a record with fields `0`, `1`, and so on. |
| one_of | A closed list of named variants. Exactly one variant is present. |
| collection | Items with laws: ordered or unique. A map keeps its own `map` kind with unique keys. |

Vocabulary follows Rust: `optional`, `none`, and `some`.

### Decomposition into paths

- A **record** decomposes into paths: `x.a`, `x.b.c`.
- A **one_of** uses the variant tag as a path segment: `x.circle.r`, or
  `x.empty: ()` for a unit variant. Exactly one variant path exists.
- A **collection** stops decomposition and keeps its laws. Each item decomposes
  again, relative to the item.
- The **type** defines the set of paths. A value never moves its path.

Paths are how validation and comparison read and report values. **The wire
format does not change**: the trace keeps the nested OTLP value tree, which
also holds `none` and the outcome channels. Path rules, such as the elided
`some` segment below, are reading rules only.

### Slot properties

Any slot (field, input, result, item, map value, or variant payload) can have:

| Property | Meaning |
|---|---|
| `type` | The concept or scalar type. |
| `optional` | The source declares that the value can be `none`. |
| `oblivious` | The source does not declare whether the value can be `none`. |

The registry keeps the existing `optional` type kind. Validation and
comparison read `optional<T>` as a slot of type `T` with `optional: true`.
`optional<optional<T>>` is an optional slot whose type is `optional<T>`.
Other sum types, including a nested `Result`, stay `tagged_union`; only
`optional` is a slot property, because C# `T?` is a property of the slot, not
a sum type.

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
| Rust `enum` with data | one_of |
| C# abstract class with `[SpecUnion(typeof(A), typeof(B))]` | one_of with exactly the listed variants |
| C# class without `[SpecUnion]`, abstract or concrete | record |
| Rust and C# tuples | record with fields `0`, `1`, … |
| Rust `Vec<T>`, C# `List<T>`, and similar | ordered collection |
| Rust and C# maps and dictionaries | `map` |

- A C# record type includes inherited public fields and properties,
  base-first (#127). Methods keep `DeclaredOnly`.
- A concrete base class stays a record, even if it acts like an enum. No
  reliable signal identifies such classes.
- `[SpecUnion]` lists its variants explicitly, as `[JsonDerivedType]` does.
  The variants must be in an assembly that the base can reference; a union
  split across assemblies is not supported. At capture, an unlisted subclass is
  a registry violation.

### Annotations

Use the existing annotations, plus `[SpecUnion]` for C#.

- **Rename:** `[SpecInput("name")]` / `#[spec_input("name")]`, and SpecEvent
  `name`.
- **Ignore:** add `Ignore = true` / `ignore` to `SpecInput`/`spec_input` and to
  SpecEvent fields. It is for context that SpecGate does not create, such as a
  `CancellationToken`, caller-info parameters, or a logger. Ignored inputs are
  not captured or compared. Replay passes the language default.
- **Union:** `[SpecUnion(params Type[])]` on a C# abstract class declares a
  one_of. This replaces the inference from all concrete subclasses.

### Inputs with defaults

A registry input can have `has_default: true`. It records that the caller can
omit the input; it does not record the default value, because defaults are
language-specific.

- C#: `ParameterInfo.HasDefaultValue`. Caller-info parameters
  (`[CallerFilePath]` and similar) are filled at the call site by the
  compiler, so replay through reflection passes the declared default.
- Rust has no default parameters. `has_default` is never set from Rust today.

### Outcomes

- Rename the outcome property `empty` to `optional`.
- Remove the rule that `empty` requires `result` (`registry.md` §4.1). No
  `result` means unit, so `Option<()>` is `optional: true` with no `result`.
- Normalize Rust nesting: `Result<Option<T>, E>` gives `result: T`,
  `optional: true`, and `errors`.

### Record and warn

Capture and validation never drop a scenario because it does not match its
registry. They record the observed value with its observed type and add a
`conformance.violation` event to the operation span.

- CTSC lists the canonical violation codes. Each code has defined parameters,
  for example `path`, `declared`, and `observed`.
- Implementations choose the message text.
- Linked validation accepts a mismatch only when a violation event declares it,
  and reports it as a warning.
- Discovery does not widen the registry. Strict golden capture still fails on
  any violation.

Violations have two causes, with separate codes:

- **Product behavior**, for example duplicate elements in a collection
  declared unique, or a duplicate map key. This is a finding about the product.
- **Annotation error**, for example a custom serializer (#116, PR #129) that
  returns a shape different from its declaration. This is a finding about the
  annotation, not the product.

Producers detect invalid values and never deduplicate them silently. A
string-keyed map is a key/value list on the wire and cannot hold a duplicate
key, so the violation event carries the key and the conflicting values. Until
record and warn is implemented, producers keep rejecting invalid values.

### Replay inputs

| Case | Action |
|---|---|
| Input is ignored | Pass the language default. The result is not affected. |
| Input only on the reference side | Skip the input. The result is **inconclusive**. |
| Input only on the replay side | Use the default if `has_default`, else `none` if `optional`; the result is **inconclusive**. Else skip the scenario, with a warning. |

Generated values (property testing) are deferred.

### Comparison report

| Class | Source |
|---|---|
| Signature difference | Registry vs registry. `oblivious` is compatible with `optional` and with required; it is never a signature difference, and only behavior is compared. This rule compares one pair at a time; it is not an equivalence and is never used for grouping or registry identity. |
| Behavior difference | Trace vs trace. |
| Registry violation | Trace vs its own registry. |
| Inconclusive | Replay used a default or skipped an input. |

The comparator compares only traces today. Signature comparison is a new
subsystem.

## Deferred

- **Semantic error names.** Rust `Result<T, E>` and C# `[SpecException]` give
  one error named `error`. A different error shows as a value difference, not
  as a different error name. Annotations for named errors come later.
- **Cross-assembly unions.** A reverse form, `[SpecVariant(typeof(Base))]` on
  the subclass, only if a real codebase needs it.

## Open questions

Items 1-9 come from review; each has a proposed answer for the owner to accept
or change.

1. **Encoding of `none`.** Proposed: keep today's `{"None":{}}` wire encoding,
   also for C# `null`. No new marker.
2. **Spoofed violations.** Proposed: linked validation recomputes violations
   and requires an exact match with the events. A missing, extra, duplicate, or
   wrongly parameterized event fails validation.
3. **Observed type in a violation.** Rust values carry no type. Proposed: the
   parameters are the path, the declared type, and the observed shape (scalar,
   record, one_of, collection, map, or none). The comparator lists violations
   but does not compare them.
4. **Exit code for inconclusive.** Today the codes are 0, 1, and 2
   (`cli.md`); this is a CLI decision. Proposed: exit 0 with an
   `inconclusive:` output line.
5. **Ignored inputs that affect behavior**, for example a cancelled
   `CancellationToken`. Proposed: `ignore` asserts that the input does not
   affect behavior. Cancellation behavior is out of scope until it is modeled.
6. **Rust `ignore`.** Replay must build the default. Proposed: discovery
   rejects `ignore` on a type without `Default`; the same applies to an ignored
   SpecEvent field when replay rebuilds the struct.
7. **`has_default` use.** Replay runs only Rust candidates, and Rust never sets
   it. Proposed: metadata only for now; replay does not use it yet.
8. **Subtype members dropped.** A derived instance in a record slot records
   only the declared members. Proposed: report a violation.
9. **Semantic error names.** Review rates this high for DSAPI: C# throws
   status-specific exceptions, and Rust returns string kinds. Proposed: keep
   deferred and file an issue.
10. Registry spelling of `oblivious`. Resolve before the first slice.
11. A variant with one payload: Rust `Set(5)` gives `x.Set.0`, and a C#
    `[SpecUnion]` variant `Set(int Value)` gives `x.Set.Value`. Either put the
    payload directly at the variant path, or report the difference and let a
    rename map it.
12. Variant and field name casing across languages.
13. Whether the `conformance.empty` event is renamed with the property.
14. The initial list of violation codes and their parameters, including
    `duplicate_element`, `duplicate_key`, and `serializer_shape_mismatch`.
15. `Option<Result<T, E>>`: keep its mapping or reject it.

## Related work

- **#116 / PR #129, custom collections.** Adds lossless native collection and
  map payloads. Compatible with this ADR. It detects invalid values and
  rejects them; this ADR later changes rejection to record and warn. #129 does
  not freeze replay, default, or optional policy.
- **DSAPI path.** Capture of the DSAPI client needs #122, #127, `ignore`, and
  outcomes with `oblivious` and `none`. Replay also needs #123 and #124.
  Signature comparison, `[SpecUnion]`, violation codes, `has_default`, and
  inconclusive replay are not on that path.

## Consequences

- Normative changes to CTSC `registry.md` §4.1 and §5, `trace.md` §8, and
  `comparison.md`.
- The `optional<T>` registry kind stays. Only its reading changes.
- Rust and C# discovery, capture, replay, validators, comparator, and goldens
  change. The hand-authored parity entries in `test/behavior/matrix.json`
  change; that is an owner decision. CTSC consumers need a migration note.
- Breaking for C# authors: an abstract `[SpecEvent]` class becomes a record
  unless it has `[SpecUnion]`. Existing fixtures add the attribute.
- Implement in slices, for example: slot properties and `none` reading;
  nested `Option`; outcomes; `oblivious`; inherited members (#127); `ignore`,
  `has_default`, and replay inputs; record and warn; `[SpecUnion]`, which
  reports unlisted subclasses as violations.

## Validation

Focused registry, trace, capture, replay, validator, and comparator tests, plus
regenerated goldens with cross-language parity rows for `Option<T>` / `T?`,
`Option<Option<T>>`, `Option<()>`, `Result<Option<T>, E>`, oblivious
references, inherited members, `[SpecUnion]` unions, and registry violations.

## Supersedes / superseded by

Will supersede
[`null-values-are-capture-diagnostics.md`](null-values-are-capture-diagnostics.md)
when accepted.
