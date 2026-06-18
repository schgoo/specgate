# Incremental spec implementation

Follow this workflow when the spec has changed and an implementation
already exists.

## Workflow

1. **Diff the spec against the existing code.**
   - List spec case `name`s; list existing test functions (or harness
     case results).
   - Identify: new cases, removed cases, cases whose `expected:`
     subsequence changed.

2. **Diff the annotation surface.**
   - Are there new `operation:` names? Add `#[spec_operation]` markers.
   - New `setup:` names? Add `#[spec_setup]` factories.
   - New trace names in `expected:` that don't match an existing
     `#[spec_event]` / `spec_event!()` / `#[spec_mock]`? Add the
     missing annotation.
   - Removed names? Leave the annotation in place if the symbol is
     still useful for other specs; otherwise remove it.

3. **Implement the new behaviour.** Make the new/changed cases pass.
   Don't touch code paths that no case asserts.

4. **Re-run the harness** for the whole spec — make sure no previously
   passing case regressed.

## Rules

- **Minimize churn.** Don't rewrite working code; only touch what the
  spec change requires.
- **Spec is the source of truth.** A failing case after a spec change
  means the implementation needs updating, not the spec.
- **Trust boundary.** Same as greenfield — don't read the harness's
  generated tests or trace dumps when deciding what to implement.
