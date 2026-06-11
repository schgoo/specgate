# Incremental spec implementation

Follow this workflow when an implementation already exists for the spec you're
updating. This replaces the greenfield steps in `implement-spec.md`.

## Workflow

1. **Diff the spec against existing tests**
   - List all spec case names from the `.spec.yaml` file
   - List all test function names from the existing test file(s)
   - Identify: new cases (in spec, not in tests), removed cases (in tests,
     not in spec), and potentially changed cases (same name, different
     expected values)

2. **Diff the spec types against existing types**
   - Compare the spec's `types:` and `outputs:` against existing Rust/C# types
   - Note added fields, removed fields, renamed fields, changed type expressions
   - Note added or removed outcome variants

3. **Update types first**
   - Add new fields/variants to existing structs/enums
   - Remove fields/variants that are no longer in the spec
   - Update type expressions that changed
   - Compile to verify type changes don't break existing code

4. **Update tests (TDD)**
   - **New cases**: add test functions — write them first, watch them fail
   - **Removed cases**: delete the corresponding test functions
   - **Changed cases**: update expected values in existing test functions
   - Do NOT rewrite tests that haven't changed

5. **Update implementation**
   - Implement new logic needed by new/changed cases
   - Remove dead code from removed cases
   - Run all tests — new and existing — until they pass

6. **Update internal tests**
   - Add unit tests for any new helper functions
   - Remove unit tests for deleted helpers
   - Verify existing internal tests still pass

7. **Coverage**
   - Run coverage tool (see `implement-spec.md` for commands)
   - Add tests for any newly uncovered branches
   - Report final coverage percentage

## Rules

- **Minimize churn** — don't rewrite working code. Only touch what the spec
  change requires.
- **Spec is the source of truth** — if a test fails after a spec change, update
  the implementation, not the spec.
- **Preserve test isolation** — new tests should not depend on or interfere
  with existing tests.
- **Keep the same project structure** — don't reorganize modules or move files
  unless the spec change requires it.

## Checklist before finishing

- [ ] All new spec cases have corresponding test functions
- [ ] All removed spec cases have had their test functions deleted
- [ ] Changed spec cases have updated expected values
- [ ] Types match the current spec (fields, variants, type expressions)
- [ ] All tests pass (`cargo test` / `dotnet test`)
- [ ] Coverage measured and reported (target: 100% line coverage)
- [ ] No dead code from removed cases remains
