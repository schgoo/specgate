# SpecGate

[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](./LICENSE-MIT)

Deterministic spec-based verification for LLM-implemented code.

Engineers write specs. LLMs implement them. SpecGate closes the gap by providing
a non-stochastic harness that validates implementations against specs using
runtime traces.

## Crates

| Crate | Description |
|-------|-------------|
| [`specgate`](./rust/crates/specgate/README.md) | Umbrella crate — annotations + harness in one dependency |
| [`specgate-cli`](./rust/crates/specgate-cli/README.md) | CLI: `specgate validate` and `specgate run` |
| [`specgate-harness`](./rust/crates/specgate-harness/README.md) | Test harness: codegen, trace collection, matching |
| [`specgate-annotations`](./rust/crates/specgate-annotations/README.md) | Annotation facade |
| [`specgate-annotations-macros`](./rust/crates/specgate-annotations-macros/README.md) | Proc macros |
| [`specgate-runtime`](./rust/crates/specgate-runtime/README.md) | Runtime trace buffer |
| [`specgate-types`](./rust/crates/specgate-types/README.md) | Spec/binding parsing |

## How It Works

```
Spec YAML  +  Annotated Source  →  Harness  →  Pass / Fail per case
```

1. Write a spec (`.spec.yaml`) declaring operations, types, and expected behavior
2. Annotate source with `#[spec_operation]`, `#[derive(SpecEvent)]`, `spec_trace!`
3. The harness generates a runner, compiles it, collects traces, and compares

See the [`specgate` crate docs](./rust/crates/specgate/README.md) for usage,
and the [CLI docs](./rust/crates/specgate-cli/README.md) for command-line usage.

## Design Principles

1. **Spec is the single source of truth**
2. **Conformance checking is deterministic** — no LLM in the verification loop
3. **Traces are the evidence** — generated tests have zero domain knowledge
4. **Zero-cost in production** — annotations are no-ops without the `trace` feature

## Documentation

- [`docs/knowledge/`](./docs/knowledge/) — reference docs for spec format, annotations, bindings, etc.
- [`CHANGELOG.md`](./CHANGELOG.md) — release history

### Implementation Skill (for AI agents)

[`.github/skills/implement-spec.md`](./.github/skills/implement-spec.md) is a structured
workflow that AI coding agents follow when implementing from a spec. It defines:

- **How to read a spec** — what each field means, how types map to code
- **The TDD workflow** — annotate first, write harness bootstrap test, then implement
- **Validation gates** — `specgate validate` before implementing, `specgate run` after
- **Trust boundary** — agents must never read harness-generated artifacts as implementation input
- **Checklist** — what must be true before an implementation is considered done

Point any agent at this file when asking it to "implement a spec" or "build from spec".
The knowledge base in `docs/knowledge/` provides topic-specific reference (spec format,
annotation syntax, binding files, Rust/C# conventions, etc.).

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

[MIT](./LICENSE-MIT) OR [Apache-2.0](./LICENSE-APACHE)
