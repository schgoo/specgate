---
name: author-spec
description: >
  Authors SpecGate spec files from requirements, API descriptions, or existing
  code. Produces well-formed .spec.yaml files with operations, types, and test
  cases. Use when asked to "write a spec", "create a spec", "spec this out",
  or when a new component needs a spec before implementation.
---

# SpecGate Spec Authoring Skill

You are authoring a SpecGate spec file. Your job is to produce a well-formed
`.spec.yaml` that fully describes the component's expected behavior.

## Before you start

1. Read `docs/knowledge/authoring.md` for the authoring tutorial
2. Read `docs/knowledge/spec-format.md` for the exhaustive field reference
3. Look at existing specs under `test/rust/crates/specgate-fixtures/specs/`
   for canonical examples of every pattern

## Workflow

1. **Understand the component** — what operations does it expose? What types
   does it use? What are the success and failure paths?

2. **Declare operations** — each public function/method that the spec covers
   becomes an operation with declared inputs and outputs

3. **Define types** — complex input/output types go in the `types:` block

4. **Write cases** — cover:
   - Happy path (normal inputs → expected outputs)
   - Edge cases (empty inputs, boundaries, zero values)
   - Error paths (invalid inputs → error responses)
   - For each MUST requirement: at least one positive AND one negative case

5. **Add property tests** — for algebraic properties (commutativity,
   associativity, idempotency, round-trip) use `kind: property`

6. **Validate** — run `specgate validate` on the result

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

- `spec_version` must match the current schema (check `spec-schema.json` for the value)
- Every runnable case needs `name`, `desc`, `operation`, and `expected`
- Operation names must be snake_case
- Case names must be snake_case and unique within the file
- Expected assertions use `$` prefix for harness directives: `$result`,
  `$run`, `$unordered`, `$anywhere`
- User-defined event names are bare (no `$` prefix)
- All values in expected are strings (stringified comparison)
- Type references use `{ type: list, items: T }` syntax, not `T[]`

### Primitives vs structured types

**Default to decomposed primitives.** The spec is a behavioral contract,
not a type system. Do NOT wrap inputs in structured types unless necessary.

- **Use primitives** when an operation takes 1-5 scalar values
- **Use structured types** ONLY for collections (list of items) or when
  the same shape is shared across multiple operations via `depends_on`
- A single-instance input with only scalar fields should be decomposed:
  `name: string, value: string` — not `member: EnumMember`
- The implementation decides its internal type structure; the spec
  describes what data flows in and out
- For Option returns: Some emits `$result` with the value, None emits
  `$result: "None"`
- For Result returns: Ok emits `$outcome: "Ok"` + `$result`, Err emits
  `$outcome: "Error"` + `$error`

## Quality checklist

- [ ] Every operation has at least one case
- [ ] Error paths are covered (not just happy paths)
- [ ] Complex inputs use the `types:` block
- [ ] Property tests cover algebraic invariants where applicable
- [ ] `specgate validate` passes with 0 errors
- [ ] Case descriptions are meaningful (not just restating the assertion)
- [ ] Operations declare all outputs that cases assert on

## After authoring

The spec is ready for implementation via the `implement-spec` skill.
Run `specgate validate <spec-dir>` to verify, then hand off to implementation.
