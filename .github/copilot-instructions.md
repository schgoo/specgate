# Copilot Instructions — SpecGate

## Implementation Rule

**All implementation changes must go through the spec implementation skill.**

Do not manually edit Rust or C# source files to add features, fix bugs, or change behavior.
The correct workflow is:

1. Update the relevant `.spec.yaml` file
2. Run the implement-spec skill on the updated spec
3. The skill generates the code changes
4. Verify via harness or unit tests

This ensures all code is traceable back to a spec and prevents bypassing the spec-first design.

Direct code edits are only acceptable for:
- Build configuration (Cargo.toml, .csproj)
- CI/CD and tooling (.github/workflows, scripts)
- Documentation (docs/, knowledge files)
- Test fixtures and integration test wiring
- Spec files, schemas, and bindings
