# SpecGate Knowledge Base

Read this first. Then read only the topics relevant to your current task.

| Topic | File | When to read |
|-------|------|--------------|
| **Authoring a spec** | [`authoring.md`](authoring.md) | Start here — tutorial for writing .spec.yaml files |
| Spec format | [`spec-format.md`](spec-format.md) | Exhaustive field reference (0.4.0 syntax, directives, property cases) |
| Annotation syntax | [`annotations.md`](annotations.md) | When placing `#[spec_*]` annotations, `spec_trace!()`, or `#[derive(SpecEvent)]` in source |
| Setup & construction | [`construction.md`](construction.md) | When the operation is a method or needs setup |
| Binding files | [`bindings.md`](bindings.md) | When writing or reading a binding YAML |
| Binding targets | [`targets.md`](targets.md) | When configuring multi-target bindings or per-case target overrides |
| Validation & failure cases | [`validation.md`](validation.md) | When debugging why a case fails to load or match |
| Rust conventions | [`rust.md`](rust.md) | When implementing in Rust |
| C# conventions | [`csharp.md`](csharp.md) | When implementing in C# |
| Greenfield workflow | [`greenfield.md`](greenfield.md) | When no implementation exists yet |
| Incremental updates | [`incremental.md`](incremental.md) | When updating an existing implementation |
| **Fixture index** | [`fixtures.md`](fixtures.md) | Canonical examples of every feature, organized by topic |

The canonical examples of every supported spec pattern live under
`test/rust/crates/specgate-fixtures/`. When this knowledge base and a
fixture disagree, the fixture is the source of truth.
