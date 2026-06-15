# Greenfield spec implementation

Follow this workflow when no implementation exists yet for the spec you're
implementing. After completing these steps, return to `implement-spec.md`
for the shared reference sections (rules, checklist, harness validation).

## Workflow

1. **Determine implementation mode**
   - Check if the SpecGate annotation package exists as a dependency or in the
     workspace (e.g., a proc macro crate for Rust, an attribute NuGet for C#)
   - If not → bootstrap mode (conventional tests, no annotations)
   - If yes → annotated mode (use `spec_operation`, `spec_setup`, etc.)

2. **Plan your implementation**
   - Map spec `types` to language types: oneof → enum, fields → struct,
     causes → error type
   - Map spec `inputs` to function/method parameters
   - Map spec `outcome` to return type (oneof → Result/enum variants)
   - Map spec `outputs` to observable fields on the return value
   - For state machine specs: map `state` to struct fields, `init` to
     constructor, `operations` to methods

3. **Scaffold the project**
   - Create crate/project following `docs/knowledge/rust.md` or `csharp.md`
   - Add to the workspace if one exists
   - Add dependencies (specgate-annotations if annotated mode)
   - Create module structure matching the spec's component name

4. **Write tests first (TDD)**
   - Create a test file (integration tests preferred)
   - Generate one test function per spec case
   - For single-operation specs: construct inputs, call the function, assert
     outcome and outputs match expected values
   - For state machine specs: create instance, execute steps in order, assert
     state and outcomes after each step (see `kinds.md`)
   - Run tests — they should all fail (red phase)

5. **Implement the component**
   - Implement types first (structs, enums)
   - Implement the entry point function/method
   - Run tests after each meaningful change — watch them turn green
   - Continue until all spec-case tests pass

6. **Add internal tests**
   - Identify code paths not reachable through spec cases
   - Add unit tests for those paths
   - Do not duplicate coverage already provided by spec-case tests

7. **Build and run all tests** to verify everything passes together

## Bootstrap mode details

When in bootstrap mode (no annotation crate available):

- **Do not use annotations** — they don't exist yet
- **Generate conventional tests** from spec cases (e.g., `#[test]` in Rust,
  `[Fact]`/`[Theory]` in C#)
- **Each spec case becomes a test function** — construct inputs, call the
  implementation, assert outputs match expected values
- **The spec's type definitions guide your Rust/C# types** — oneof → enum,
  fields → struct
- **The spec is the source of truth** — if a test fails, the implementation
  is wrong, not the spec

## Rules

- **Spec is the source of truth** — if a test fails, the implementation
  is wrong, not the spec
- **Types come from the spec** — oneof → enum, fields → struct, causes → error type
- **Every spec case gets a test** — no cases should be skipped
- **Integration tests first** — spec-case tests exercise the full component;
  unit tests fill gaps afterward
