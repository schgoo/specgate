---
name: implement-spec
description: >
  Implements a SpecGate spec file — generates source code, tests, and build
  infrastructure from a .spec.yaml file. Use when asked to "implement this spec",
  "generate code from spec", "implement <component>", or when given a .spec.yaml
  file to implement.
---

# SpecGate Spec Implementation Skill

You are implementing a SpecGate component from its spec file. Your job is to
produce working source code with tests that verify the spec's cases.

## Workflow

1. **Read the spec file** the user provides or references
2. **Read `docs/knowledge/index.md`** to see what knowledge topics are available
3. **Read only the knowledge files relevant to this spec** — don't load everything
4. **Check if implementation already exists** — look for the crate/project, existing
   test files, and source modules that correspond to this spec's component name
   - **If YES** → read `docs/knowledge/incremental.md` and follow the incremental
     workflow. Do NOT continue with the greenfield steps below.
   - **If NO** → continue with the greenfield workflow below
5. **Determine implementation mode** (see below)
6. **Plan your implementation** based on the spec's types, inputs, outputs, and cases
7. **Scaffold the project** if it doesn't exist (Cargo.toml, project structure)
8. **Write tests first (TDD)** — generate test functions from spec cases before implementing
9. **Implement** the component logic until all tests pass
10. **Add internal tests** for helper functions and edge cases not in the spec
11. **Build and run all tests** to verify

## Implementation modes

### Annotated mode (default)

Use when the SpecGate annotation crate exists for this language and the spec
has a `binding:` field. Generate code with `spec_operation`, `spec_setup`,
`spec_mock`, etc. annotations. Create a binding file if needed.

### Bootstrap mode

Use when the annotation crate does NOT exist yet (check if the proc macro
or attribute crate is available in the project's dependencies). In this mode:

- **Do not use annotations** — they don't exist yet
- **Generate conventional tests** from spec cases (e.g., `#[test]` in Rust,
  `[Fact]`/`[Theory]` in C#)
- **Each spec case becomes a test function** — construct inputs, call the
  implementation, assert outputs match expected values
- **The spec's type definitions guide your Rust/C# types** — oneof → enum,
  fields → struct
- **The spec is the source of truth** — if a test fails, the implementation
  is wrong, not the spec

How to detect bootstrap mode:
1. Check if the SpecGate annotation package exists as a dependency or in the
   workspace (e.g., a proc macro crate for Rust, an attribute NuGet for C#)
2. If not → bootstrap mode
3. If yes → annotated mode

## What the spec tells you

### Single-operation specs

- `name` — the component name (use for module/crate naming)
- `binding` — which binding file to use (optional — absent in bootstrap mode)
- `target` — how to build/run (you'll create this in the binding file)
- `inputs` — what the entry point takes
- `types` — type definitions (oneof = enum/union, fields = struct)
- `outcome` — what the operation returns (variants or single type)
- `outputs` — what's observable per outcome
- `cases` — concrete test cases (input → expected outcome + outputs)

### State machine specs

- `state` — named state variables the component holds (types match SpecCapture getters)
- `init` — initial state values (what the constructor produces)
- `operations` — named operations with their own inputs/outcomes/outputs
- `invariants` — approved invariants (properties the component must maintain)
- `cases` with `steps` — ordered operation sequences on the same component instance

A spec is one or the other — if it has `operations`, it's a state machine spec.
See `spec-format.md` for the full format and `kinds.md` for test generation patterns.

## Project scaffolding

If the project doesn't exist yet, create it following the conventions in
`docs/knowledge/rust.md` or `docs/knowledge/csharp.md`. Add the crate/project
to the workspace if one exists.

## Generating tests from spec cases (TDD)

Follow a test-driven workflow: write tests from spec cases **before** implementing.

1. **Spec-case tests come first** — each spec case becomes a test function.
   These are integration tests exercising the full component. They are your TDD
   red-green loop. Write them, watch them fail, implement until they pass.
   For state machine specs, each case follows the component lifecycle
   (create → step → assert → step → assert) — see `kinds.md` for patterns.
2. **Unit tests fill gaps** — once spec cases pass, add unit tests for code paths
   that can't be adequately tested through the integration path. If a behavior
   IS reachable through a spec case, test it there — don't duplicate coverage
   with a unit test.
3. **100% line coverage** — measure with a coverage tool, not by inspection.
   - Rust: `cargo llvm-cov --summary-only` (install with `cargo install cargo-llvm-cov` if needed)
   - C#: `dotnet test --collect:"XPlat Code Coverage"` + `reportgenerator`
   - If line coverage is below 100%, identify uncovered branches and add tests.
   - Report the final coverage percentage before declaring done.

The harness generates and runs these tests via the binding, but having them
inline in the project gives fast feedback during development.

See the language-specific knowledge files for test code examples.

## Rules

### Annotated mode only
- **Every `spec_operation` needs a `kind`** — infer from the spec's structure
- **Every `spec_setup` needs a `name`** — use the setup's purpose as the name
- **Every `spec_mock` needs a `name`** — use the dependency's logical name
- **Setup functions must not take `self`** — they are free functions

### Both modes
- **Types are suggestions, not prescriptions** — the spec says what fields must be
  available, not how to structure internals. Use idiomatic language constructs.
- **Spec cases are exhaustive** — every case must have a corresponding test
- **Tests must actually run and pass** — build and verify before finishing

### When a spec expectation seems wrong

The spec is the source of truth — if a test fails, the implementation is
wrong, not the spec. However, specs can have bugs. If you encounter a case
where:

1. The expected values are internally inconsistent (e.g., `passed: 2` but `total: 1`)
2. The expectation contradicts the spec's own type definitions or outcome variants
3. Two cases contradict each other
4. You cannot see any implementation that could produce the expected output

Then:

- **Do not silently modify the spec** — it is owned by the user
- **Do not loop endlessly** trying to make an impossible case pass
- **Flag the issue clearly** — explain what the expectation says, why you
  believe it cannot be met, and what you think the correct expectation is
- **Continue implementing other cases** — don't block all progress on one
  questionable case
- **Mark the flagged test as `#[ignore]`** with a comment explaining the
  suspected spec issue, so the rest of the suite still passes

### When a spec uses features the harness doesn't consume yet

State machine specs may use `state`, `init`, `operations`, `invariants`, or
`steps` before the harness parser or code generator understands those fields.
This is expected — the spec format is ahead of the tooling.

In this situation:

- **Implement the component to satisfy the spec's intent** — the `operations`
  and `steps` tell you what the component's API looks like and how it behaves
- **Write tests that follow the spec cases** — translate `steps` into
  sequential method calls by hand (see `kinds.md` for the pattern)
- **Do not skip or simplify cases** because the harness can't generate them yet
- **Do not remove spec fields** that the harness doesn't consume — they are
  there for future tooling and for human readers

## Knowledge base

Before implementing, read `docs/knowledge/index.md` to see available topics.
Then read only what you need:

- Always read: `spec-format.md` (understand the spec you're implementing)
- Always read: `rust.md` or `csharp.md` (whichever language you're implementing in)
- Read if placing annotations: `annotations.md`
- Read if kind is not Stateless: `kinds.md`
- Read if spec has `operations` section: `kinds.md` (StateMachine test patterns)
- Read if entry point is a method: `construction.md`
- Read if creating binding: `bindings.md`
- Read if you need validation rules: `validation.md`

## Checklist before finishing

- [ ] Project scaffolded with correct structure
- [ ] All spec types implemented as idiomatic language types
- [ ] Every spec case has a corresponding test function
- [ ] Core logic implemented — all spec-case tests pass
- [ ] Unit tests added for code paths not reachable through spec cases
- [ ] Tests build and pass (`cargo test` / `dotnet test`)
- [ ] Coverage measured and reported (target: 100% line coverage)
- [ ] Uncovered branches identified and tested, or justified if untestable
- [ ] If annotated mode: annotations present, binding file created
- [ ] If bootstrap mode: conventional tests cover all cases
- [ ] **Spec harness validation** — run the spec through the harness (see below)

## Running the spec harness

After implementation passes all tests, validate that the spec harness can
generate and run the tests deterministically:

1. **Check if a binding exists** for this spec — look at the spec's `binding:`
   field and the corresponding `bindings/<name>.yaml` file
2. **Check if the binding defines targets** — if the spec has `target: test`
   (or similar), the binding should have a matching `targets.test` entry
3. **If both exist**, run the harness:
   - Rust: `cargo run -p specgate-cli -- run <spec-file>` (if CLI exists)
   - Or programmatically: create a test that calls `Harness::run_spec("<spec-file>")`
     with the appropriate backend registered
4. **If the harness run succeeds**, all spec cases should produce `pass` results
5. **If the harness or CLI doesn't exist yet** (bootstrap phase), skip this step
   but note it as a follow-up

The point: the spec harness should be able to generate and execute the same
tests you wrote by hand. If it can't, something is missing from the spec,
binding, or backend.
