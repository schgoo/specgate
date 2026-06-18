# Greenfield spec implementation

Follow this workflow when no implementation exists yet for the spec
you're implementing.

## Workflow

1. **Read the spec.**
   - List the `cases`. Each case is a test you must make pass.
   - For each case note: `setup` (if any), `operation` (or `steps`),
     `inputs`, and the full `expected:` subsequence.

2. **Read the binding.** It tells you the language and where the
   package lives (`package_root`). If the package doesn't exist, create
   it as a sibling of (or at) that path.

3. **Bring in the annotation crate.**
   - Rust: add `specgate-annotations` as a dependency.
   - C#: add the SpecGate annotations package (planned).

4. **Map spec → code.**
   - One `#[spec_operation("<name>")]` per operation referenced by a
     `operation:` or `steps[].operation:` field.
   - One `#[spec_setup("<name>")]` per setup referenced by `setup:`.
   - One `#[spec_event]` per field whose value the spec asserts via a
     `- <field>: …` entry.
   - One `spec_event!("<name>", expr)` per inline checkpoint the spec
     asserts (e.g. `after_upper` in `checkpoint_inline`).
   - One `#[spec_mock("<name>")]` per mock referenced from the case's
     `inputs.<mock>` table.

5. **Implement just enough** that the operation runs end-to-end. Don't
   add behaviour that no `expected:` entry asserts — the trust boundary
   forbids implementing toward the validation output.

6. **Run the harness.** Iterate until every case passes.

## Rules

- **Spec is the source of truth.** If a case fails, the implementation
  is wrong (or the annotations are missing) — not the spec.
- **Don't read validation artifacts.** The trust boundary (design
  requirement 11) says the implementation must come from the spec, the
  binding, and the existing code — never from harness-generated tests
  or trace dumps.
- **Use trace names by convention.** `<op>.result`, `<op>.outcome`,
  `<op>.error`, bare `<field>`, `<mock>.request`, `<mock>.response`,
  `<setup>.<param>`. See `spec-format.md`.
