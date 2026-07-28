---
name: author-spec
description: >
  Authors SpecGate spec files from requirements, API descriptions, or existing
  code. Produces well-formed .spec.yaml files with operations, types, and test
  cases. Use when asked to "write a spec", "create a spec", "spec this out",
  or when a new component needs a spec before implementation.
---

# SpecGate Spec Authoring Skill

## Outcome

A well-formed, `validate`-clean `.spec.yaml` that fully describes a component's
behavior and serves as the **durable, reviewable scope boundary** for
implementation. The spec — not any conversation about it — is the contract the
`implement-spec` skill and the harness treat as the source of truth.

## Prerequisites

- Read `docs/knowledge/authoring.md` (authoring tutorial) and
  `docs/knowledge/spec-format.md` (exhaustive field reference).
- Skim existing example specs for canonical patterns — in the SpecGate repo
  these live under `test/rust/crates/specgate-fixtures/specs/`; in a downstream
  project, your own `specs/` directory.
- Know the current `spec_version` (check `spec-schema.json`).

## Principles

- **The spec is the reviewable artifact.** It is the prompt and scope fence for
  whoever implements it. Anything a case does not assert is out of scope — the
  implementer will not infer it, and absence from the spec is treated as an
  intentional decision, not an omission.
- **Cases must discriminate.** Every case should fail if the behavior it
  describes is absent or wrong. A case that also passes against an empty or
  trivial implementation proves nothing (see "Discriminating cases").
- **Cover a required set of states, not an ad-hoc few.** Enumerate the states in
  step 4 for each operation rather than picking cases by intuition.
- **Preview before handing off.** `specgate validate --spec-only` is a dry-run
  gate you run before any implementation exists; the spec must be clean before
  handoff.

## Workflow

1. **Understand the component** — what operations it exposes, what types it
   uses, its success and failure paths, and any shared state boundary.

2. **Declare operations** — each public function/method the spec covers becomes
   an operation with declared inputs and outputs.

3. **Define types** — complex input/output types go in the `types:` block.

4. **Write cases — cover the required state set.** For each operation, include a
   case for every state that applies:
   - **Happy** — normal inputs → expected outputs.
   - **Empty / boundary** — empty collections, zero, min/max, first/last.
   - **Error** — invalid inputs routed to the declared error/fault channel.
   - **Absent (when the return type implies it)** — `None` for an `Option`
     return, the `Err` arm for a `Result`, a fault for a panicking path.
   - For every **MUST** requirement: at least one positive AND one discriminating
     negative case.

5. **Add property tests** — for algebraic properties (commutativity,
   associativity, idempotency, round-trip) use `kind: property`.

6. **Validate (dry-run gate)** — run `specgate validate <spec-dir> --spec-only`
   and fix every error before handing off.

## Discriminating cases

A case is *discriminating* when it fails against a wrong or empty
implementation. Prefer:

- **Asymmetric expected values** — `add(2, 3) → 5` discriminates; `add(2, 2) → 4`
  is also satisfied by multiplication. Choose inputs whose expected output is
  unique to the correct behavior.
- **Distinct error text or shape** on failure paths, so an `Err`/error case
  cannot be satisfied by the success path (or vice versa).
- **A negative case per MUST** that passes only if the requirement is actually
  enforced.

## Spec structure

```yaml
spec_version: "0.4.0"
name: <dotted.component.name>
binding: <path/to/binding.yaml>

types:
  # Named struct/enum types used in operations

operations:
  # Named operations with inputs/outputs

cases:
  # Test cases exercising the operations
```

## Rules

- `spec_version` must match the current schema (check `spec-schema.json` for the value).
- Every runnable case needs `name`, `desc`, `operation`, and `expected`.
- Operation names must be snake_case.
- Case names must be snake_case and unique within the file.
- Expected assertions use `$` prefix for harness directives: `$result`,
  `$run`, `$unordered`, `$anywhere`, `$fault`.
- User-defined event names are bare (no `$` prefix).
- All values in expected are strings (stringified comparison).
- Type references use `{ type: list, items: T }` syntax, not `T[]`.

### Primitives vs structured types

**Default to decomposed primitives.** The spec is a behavioral contract,
not a type system. Do NOT wrap inputs in structured types unless necessary.

- **Use primitives** when an operation takes 1-5 scalar values.
- **Use structured types** ONLY for collections (list of items) or when the same
  shape is shared across multiple operations via `depends_on`.
- A single-instance input with only scalar fields should be decomposed:
  `name: string, value: string` — not `member: EnumMember`.
- The implementation decides its internal type structure; the spec describes
  what data flows in and out.

### Outcome shapes (Option / Result / fault)

The outcome of a variant-returning operation is emitted as a tagged-variant
`$result` map; a panic/unrecoverable path is emitted as `$fault`:

- `Option<T>` — `$result: { Some: <value> }` or `$result: { None: {} }`.
- `Result<T, E>` — `$result: { Ok: <value> }` or `$result: { Err: <message> }`.
- Panic / unrecoverable — `$fault: <message>` (undeclared by definition; assert
  it only in the case where it occurs).

See `result_ok.spec.yaml`, `result_err.spec.yaml`, `option_some.spec.yaml`, and
`option_none.spec.yaml` (the reference specs shipped with SpecGate, under
`test/rust/crates/specgate-fixtures/specs/`) for the canonical shapes, and
`docs/knowledge/csharp.md` / `rust.md` for how each language realizes them.

## Verification

Before handing off to implementation:

- [ ] `specgate validate <spec-dir> --spec-only` passes with 0 errors.
- [ ] Every operation has at least one case.
- [ ] The required state set (happy / empty-boundary / error / absent) is covered
      per operation where applicable.
- [ ] Every MUST has a positive AND a discriminating negative case.
- [ ] Each case is discriminating (would fail against an empty implementation).
- [ ] Complex inputs use the `types:` block.
- [ ] Property tests cover algebraic invariants where applicable.
- [ ] Case descriptions explain intent, not just restate the assertion.
- [ ] Operations declare all outputs their cases assert on.

## Handoff

The spec is the durable artifact the `implement-spec` skill consumes. Hand off
only once `validate --spec-only` is clean. The implementer treats the spec as
read-only and authoritative — anything left ambiguous or unasserted will not be
implemented.

## Troubleshooting

- **`spec file is not valid YAML`** — a common cause is an unquoted `desc:` value
  containing an inner `': '` (colon-space), which YAML parses as a nested
  mapping. Quote the value or rephrase to remove the colon.
- **`validate` reports an operation/target/binding error under `--spec-only`** —
  these structural checks run even without source; fix the reference before
  handoff.
- **A case cannot be made discriminating** — if the only expected value is
  symmetric (satisfiable by a wrong implementation), add a second case with an
  asymmetric input, or assert an additional observable event.
