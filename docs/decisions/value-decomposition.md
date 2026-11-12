# Language-neutral value decomposition

> **Status:** accepted; implementation pending
> **Owner:** @schgoo
> **Date:** 2026-11-04
> **Tracking:** #125, #127

The owner accepted this design on 2026-11-12, including the specified CTSC,
CLI, replay, and golden-matrix changes. Implementation follows the bounded
slices in the [value-model roadmap](../contributor/roadmap.md#value-model).
This acceptance does not claim that the behavior is already implemented.
The inherited-member slice (#127) is now implemented; the other value-model
slices, including type aliases (#122), remain pending.
For the future coordinated wrapper slice, the owner approved a joint cutover to
CTSC `0.5.0` and `ctsc.strict/0.3.0`, requiring recapture with no legacy reader.
The inheritance-only slice leaves current contract and policy versions unchanged.

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
- C# record types previously omitted inherited members (#127, now delivered).
- A Rust `Err` nested in a value, such as `Option<Result<T, E>>`, reads
  differently from a top-level `Err` or a C# exception.

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
  `x.empty: ()` for a unit variant. A record with no fields reads like unit,
  so `A {}` gives `x.A: ()`. Exactly one variant path exists.
- A variant payload is read faithfully. Rust `Count(5)` gives `x.Count.0`,
  and a C# variant class `Count` with property `Value` gives
  `x.Count.Value`. The `.0` segment is never elided; the comparator reports
  the difference.
- A **collection** stops decomposition and keeps its laws. Each item decomposes
  again, relative to the item.
- The **type** defines the set of paths. A value never moves its path.

Paths are how validation and comparison read and report values. **The wire
format does not change**: the trace keeps the nested OTLP value tree, which
also holds `none` and the outcome channels. Path rules, such as the elided
`some` segment below, are reading rules only.

The nested typed tree is authoritative; there is no wire flattening. Shortened
display labels must never merge distinct states. Ambiguous literal names use
quoted bracket notation: a real field `["err"]` differs from the synthetic
`.err` segment. These are owner-approved requirements for the future wrapper
slice, not changes delivered by #127.

### Slot properties

Any slot (field, input, result, item, map value, or variant payload) can have:

| Property | Meaning |
|---|---|
| `type` | The concept or scalar type. |
| `optional` | The source declares that the value can be `none`. |
| `oblivious` | The source does not declare whether the value can be `none`. |
| `errors` | The value can be an error instead of a `type` value. |

The registry keeps the existing `optional` type kind. Validation and
comparison read `optional<T>` as a slot of type `T` with `optional: true`.
`oblivious` is a flag on that kind:
`{"kind":"optional","value":T,"oblivious":true}`. A new `fallible` kind,
`{"kind":"fallible","value":T,"errors":[...]}`, carries `errors` the same
way. Each wrapper level is its own slot, so `optional<optional<T>>` is an
optional slot whose type is `optional<T>`, and the registry keeps the order of
`Option<Result<T, E>>` and `Result<Option<T>, E>`. Signature comparison
reports a different order. Other sum types are one_of.

On the wire, `optional` and `oblivious` slots use the same encoding:
`{"Some": v}` or `{"None": {}}`. C# `T?` already uses it. The path reading
elides the innermost `Some`, so a C# oblivious `{"Some": 5}` and a Rust
required `5` both read as `x = 5`. `oblivious` differs from `optional` only
in registry compatibility (see Comparison report).

`none` is a distinct value. It is not unit and it is not absence:

| `Option<()>` | Path value |
|---|---|
| `None` | `x: none` |
| `Some(())` | `x: ()` |

`none` at a record slot is different from `none` in its fields:
`x: none` and `x.a: none` are different paths.

### Errors

An error reads as an `err` segment; `Ok` is elided. The wire format keeps
`{"Ok": v}` and `{"Err": e}` for a nested `Result`, and the error outcome
event at the top level.

| `Result<i32, E>` | Path value |
|---|---|
| `Ok(5)` | `x: 5` |
| `Err(e)` | `x.err: e` |

The top-level error outcome, from a Rust `Err` or a C# exception, reads as
`result.err: e`. A nested `Err` reads the same, so `Some(Err(e))` in
`Option<Result<T, E>>` compares equal to a C# exception.

### Nested wrappers

`Some` and `Ok` are elided unless two states would then read the same. Every
state has a distinct path, so nothing is lost.

- An `Option` directly in an `Option` adds one `some` segment.
- A `Result` directly in a `Result` adds one `ok` segment.
- The illustrated two-wrapper mixed cases need no additional segment:
  `none` and `err` do not collide. Repeated kinds separated by other wrappers
  must still retain distinct states; this does not prescribe a generalized
  shortening algorithm.

| `Option<Option<i32>>` | Path value |
|---|---|
| `None` | `x: none` |
| `Some(None)` | `x.some: none` |
| `Some(Some(5))` | `x.some: 5` |

| `Result<Result<i32, E1>, E2>` | Path value |
|---|---|
| `Err(e2)` | `x.err: e2` |
| `Ok(Err(e1))` | `x.ok.err: e1` |
| `Ok(Ok(5))` | `x.ok: 5` |

`Option<Result<T, E>>` and `Result<Option<T>, E>` read the same: `x: v`,
`x: none`, and `x.err: e`. Only their registries differ.

C# `T?` cannot nest. C# can nest `SpecGate.Annotations.Option<T>`, which uses
the same encoding. C# also has the existing public
`SpecGate.Annotations.Result<T, E>` helper. The owner confirmed on 2026-11-12
that its support is preserved and its recorded meaning must match Rust
`Result` in the future coordinated wrapper slice; #127 does not change it.

### Language mapping

| Native shape | Concept |
|---|---|
| Rust `Option<T>`; C# `T?`, `Nullable<T>`, `SpecGate.Annotations.Option<T>` | `optional` slot |
| C# reference type without a nullable context | `oblivious` slot |
| C# `null` in an `optional` or `oblivious` slot | `none` |
| Rust `Result<T, E>` | `fallible` slot |
| Rust `enum` with data | one_of |
| C# abstract `[SpecEvent]` class | one_of of the concrete subclasses that discovery sees |
| C# concrete class | record |
| Rust and C# tuples | record with fields `0`, `1`, … |
| Rust `Vec<T>`, C# `List<T>`, and similar | ordered collection |
| Rust and C# maps and dictionaries | `map` |

- A C# record type includes inherited public fields and properties,
  base-first (#127). Methods keep `DeclaredOnly`.
- A concrete base class stays a record, even if it acts like an enum. No
  reliable signal identifies such classes.
- Union inference does not change: discovery makes an abstract `[SpecEvent]`
  class a one_of of the concrete subclasses it sees. No new attribute is
  added. At capture, a subclass that discovery did not see is recorded as
  observed; validation reports it as an `undeclared_variant` violation.
- A derived instance in a record slot is recorded as it is: the declared
  members plus the extra members of its runtime type. Validation reports each
  extra member as an `undeclared_member` violation.

#### Inherited-member selection (#127, owner-approved 2026-11-12)

Discovery and capture select effective public instance fields and properties
across the hierarchy **by CLR/source member name first**, before applying the
existing annotation/default-selection rules. Walk base-first, properties before
fields within each declaring type. If `Child` replaces `Parent.Name`, use
`Child.Name` once in `Parent.Name`'s original position. This applies to both
virtual overrides and `new` hiding, even when the child changes the semantic
rename or type. Preserve the selected member's actual declaration nullability
and recursively queue its referenced annotated types.

An unannotated child follows the existing non-inheriting member-annotation
lookup; this decision adds no attribute-inheritance semantics. Ordinary
unannotated members are never silently selected. Static/private members and
ordinary properties without public non-indexed getters are excluded. Property
setups use the **same effective scope** for detecting explicit annotations and
selecting members: inherited explicit annotations prevent all-member inference.
Selected setup properties still require public non-indexed getters.
Distinct effective source members whose final semantic names collide reject;
replacement is not semantic-name deduplication.

This rule is delivered for ordinary records, property setups, and inherited
union payload fields. Operation/setup method discovery remains `DeclaredOnly`.
It does not implement runtime-subtype extra-member observation or the
record-and-warn validation changes described above.

### Annotations

Use the existing annotations. No new attribute is added.

- **Rename:** `[SpecInput("name")]` / `#[spec_input("name")]`, and SpecEvent
  `name`.
- **Names:** Field, variant, and input names are the source names. Neither
  language converts case. Authors align names with the rename annotations;
  the comparator reports names that differ.
- **Ignore:** add `Ignore = true` / `ignore` to `SpecInput`/`spec_input` and to
  SpecEvent fields. It is for context that SpecGate does not create, such as a
  `CancellationToken`, caller-info parameters, or a logger. Ignored inputs are
  not captured or compared. Replay passes the language default.
  `ignore` is the author's assertion that the input does not affect the
  captured behavior. Cancellation is not modeled: if a test cancels an ignored
  token, the comparator reports a behavior difference with no further cause.
  In Rust, `ignore` requires `T: Default`; the macro enforces this at compile
  time, for inputs and for ignored SpecEvent fields.

### Inputs with defaults

A registry input can have `has_default: true`. It records that the caller can
omit the input; it does not record the default value, because defaults are
language-specific.

- C#: `ParameterInfo.HasDefaultValue`. Caller-info parameters
  (`[CallerFilePath]` and similar) are filled at the call site by the
  compiler, so replay through reflection passes the declared default.
- Rust has no default parameters. `has_default` is never set from Rust today.
- `has_default` is registry metadata only. Signature comparison shows it.
  Replay runs only Rust candidates, so replay uses it only when C# replay
  exists.

### Outcomes

The outcome is the result slot. Its properties describe the outermost wrapper
of the return type; inner wrappers stay `optional` or `fallible` kinds in
`result`.

- Rename the outcome property `empty` to `optional`, and the
  `conformance.empty` event to `conformance.none` (`trace.md` §7.3).
- Add the outcome property `oblivious` next to `optional`.
- Remove the rule that `empty` requires `result` (`registry.md` §4.1). No
  `result` means unit, so `Option<()>` is `optional: true` with no `result`.
- `Result<Option<T>, E>` gives `errors` and `result: optional<T>`.
  `Option<Result<T, E>>` gives `optional: true` and `result: fallible<T>`.
  Both read the same (see Nested wrappers).

### Record and warn

The trace is raw observed output. Capture never drops a scenario because a
value does not match its registry; it records the value as observed and adds
no violation marker.

- Only `compare` and `validate linked` check values against the registry.
  `validate linked` reports violations as warnings; the comparator reports
  them in the registry violation class. Both recompute violations from the
  trace and the registry, and neither trusts anything in the trace about
  violations.
- Capture and replay do not check values against the registry. The trace still
  names its registry by id, version, and digest. Capture and replay run only
  the structural trace validation; a failure there is a SpecGate defect and
  stops the command. A product-behavior violation is therefore visible only
  after `compare` or `validate linked`.
- CTSC lists the canonical violation codes and their parameters in the
  validation and comparison contract, not in `trace.md`. Each violation has a
  `code` and a `path`:

  | Code | Parameters | Meaning |
  |---|---|---|
  | `type_mismatch` | `declared`, `observed` | Wrong shape or scalar, or `none` in a slot that is not `optional` or `oblivious`. `observed` is `scalar`, `record`, `one_of`, `collection`, `map`, or `none`. |
  | `missing_member` | `member` | A declared field or tuple element is absent. |
  | `undeclared_member` | `member` | A field or tuple element is not declared. |
  | `undeclared_variant` | `variant` | A variant is not declared. |
  | `undeclared_outcome` | `outcome` | An outcome is not declared. |
  | `undeclared_observation` | `name` | An observation is not declared. |

  These are not violations: a duplicate (an annotation error), and an unknown
  type or component (a registry defect).
- Implementations choose the message text.
- Discovery does not widen the registry. The golden check keeps linked
  validation and still fails on any violation.

Record and warn covers product behavior only. A value that breaks its own
declaration is an **annotation error**, and capture stops with an error that
names the path:

- a duplicate key in a value declared as a map, or a duplicate element in a
  collection declared unique (native maps and sets cannot hold duplicates, so
  these come only from projection or a custom serializer);
- a custom serializer (#116, PR #129) that returns a shape different from its
  declaration.

Producers never deduplicate silently.

### Replay inputs

| Case | Action |
|---|---|
| Input is ignored | Pass the language default. The result is not affected. |
| Input only on the reference side | Skip the input. The result is **inconclusive**. |
| Input only on the replay side | Use the default if `has_default`, else `none` if `optional`; the result is **inconclusive**. Else skip the scenario, with a warning. |

Generated values (property testing) are deferred.

`replay` only runs the candidate and records what happened. It does not judge
the result. It writes the candidate trace plus a **link file** in SpecGate
format, documented with the capture manifest in `docs/reference/bundle.md`
(not CTSC). The link file records:

- the SHA-256 digests of the reference bundle manifest, registry, and trace;
- the candidate identity and language, and the SpecGate version;
- each input that replay defaulted or skipped.

`compare` reads the link file, checks the digests, and classifies each
scenario, including **inconclusive**. If a digest does not match the
reference bundle, the inputs do not belong together: `compare` cannot run,
exits 1, and names each digest that does not match.

### Exit codes and reports

`replay`, `compare`, and `validate` exit 1 only when they cannot run, for
example when an input file cannot be read. An invalid artifact, a mismatch, a
violation, or an inconclusive scenario is a finding in the command's report,
not an exit code. CI gates on the report. This changes the CLI:

- `compare` and `validate` write a stable, machine-readable report in SpecGate
  format. The CTSC comparison result (`comparison.md` §6) defines the content,
  not the serialization.
- The link file and the reports are JSON with a `schemaVersion` field. Each
  has a JSON Schema in `docs/reference/`, and a test validates the output
  against it. The slice that adds each file defines its fields.
- `scripts/ctsc-smoke.ps1` expects `compare` to exit 1 on a mismatch; it
  changes.
- Capture and replay stop running linked validation; today a linked
  validation failure fails both (`capture/encoding.rs`, `replay/output.rs`).
  The golden check calls linked validation as a library and keeps it.

### Comparison report

| Class | Source |
|---|---|
| Signature difference | Registry vs registry. `oblivious` is compatible with `optional` and with required; it is never a signature difference, and only behavior is compared. This rule compares one pair at a time; it is not an equivalence and is never used for grouping or registry identity. |
| Behavior difference | Trace vs trace. |
| Registry violation | Trace vs its own registry. |
| Inconclusive | The link file shows that replay used a default or skipped an input. |

The comparator lists each side's violations but does not compare them; a
mismatch already shows as a behavior difference at the same path.

The comparator compares only traces today. Signature comparison is a new
subsystem.

## Deferred

- **Semantic error names (#130).** Rust `Result<T, E>` and C#
  `[SpecException]` give one error named `error`. Its value is the C#
  `exception.Message` or the Rust projection of `E`. A different error shows
  as a value difference, not as a different error name, and message text often
  differs between languages. #130 also records the C# exception type.
- **Explicit unions.** An attribute that lists or closes C# variants, only if
  a real codebase needs it.
- **Name and payload mapping.** Annotations that map a variant payload or a
  name beyond the existing renames.

## Related work

- **#116 / PR #129, custom collections.** Adds lossless native collection and
  map payloads. Compatible with this ADR. A duplicate key or element is an
  annotation error, so #129's capture rejection stays. #129 does not freeze
  replay, default, or optional policy.
- **DSAPI path.** Capture of the DSAPI client needs #122, #127, `ignore`, and
  outcomes with `oblivious` and `none`. Replay also needs #123 and #124.
  Signature comparison, violation codes, `has_default`, and inconclusive
  replay are not on that path.

## Consequences

- Normative changes to CTSC `registry.md` §4.1 and §5 (the `oblivious` flag
  and the `fallible` kind) and `comparison.md`. `trace.md` §7.3 renames the
  `conformance.empty` event to `conformance.none`; the value encoding does
  not change.
- CLI changes: exit codes, the replay link file, and stable `compare` and
  `validate` reports.
- The `optional<T>` registry kind stays. Only its reading changes.
- Rust and C# discovery, capture, replay, validators, comparator, and goldens
  change. The hand-authored parity entries in `test/behavior/matrix.json`
  change; that is an owner decision. CTSC consumers need a migration note.
- No new C# attribute. Union inference stays; an unseen subclass becomes a
  violation instead of a capture failure.
- Implement in slices, for example: slot properties and `none` reading;
  nested wrappers and `fallible`; outcomes; `oblivious`; inherited members
  (#127); `ignore`, `has_default`, and replay inputs; exit codes, link file,
  and reports; record and warn with violation codes.

## Validation

Focused registry, trace, capture, replay, validator, and comparator tests, plus
regenerated goldens with cross-language parity rows for `Option<T>` / `T?`,
`Option<Option<T>>`, `Option<()>`, `Result<Option<T>, E>`,
`Option<Result<T, E>>`, `Result<Result<T, E1>, E2>`, oblivious references,
inherited members, inferred unions, and registry violations.

## Supersedes / superseded by

Supersedes the design in
[`null-values-are-capture-diagnostics.md`](null-values-are-capture-diagnostics.md)
as the implementation slices land. Until then, current behavior remains
documented in [`../status.md`](../status.md) and
[`../reference/limitations.md`](../reference/limitations.md).
