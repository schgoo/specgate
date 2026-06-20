# Copilot Instructions — SpecGate

## Implementation Rule

**All implementation changes must go through the spec implementation skill.**

The skill is at `.github/skills/implement-spec.md`. When asked to "implement
a spec", "implement the spec", "build from spec", or similar, always follow
that skill's workflow.

When delegating to a subagent, always include this reference in the prompt:
> Follow the implementation skill at `.github/skills/implement-spec.md`

Do not manually edit Rust or C# source files to add features, fix bugs, or change behavior.
The correct workflow is:

1. Update the relevant `.spec.yaml` file
2. Launch the implement-spec skill as a subagent (using the task tool) on the updated spec
3. The subagent generates the code changes
4. Review the subagent's output and verify via harness or unit tests

Never write or edit implementation code directly — always delegate to a subagent.
This ensures all code is traceable back to a spec and prevents bypassing the spec-first design.

Direct edits (without a subagent) are only acceptable for:
- Build configuration (Cargo.toml, .csproj)
- CI/CD and tooling (.github/workflows, scripts)
- Documentation (docs/, knowledge files)
- Spec files, schemas, and bindings
- Test fixtures and integration test wiring
