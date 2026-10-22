# SpecGate documentation

This page is the documentation map for people using SpecGate, contributors, and
coding agents.

## Guides

Task-oriented instructions. Start at [`guides/README.md`](guides/README.md); it
includes a "which guide" table and the agent quick path.

- [`guides/install.md`](guides/install.md) — install the CLI, the Rust crate,
  and the C# annotations.
- [`guides/instrument-rust-crate.md`](guides/instrument-rust-crate.md) and
  [`guides/instrument-csharp-assembly.md`](guides/instrument-csharp-assembly.md)
  — annotate a component.
- [`guides/write-a-binding.md`](guides/write-a-binding.md) — describe where the
  sources and tests live.
- [`guides/capture-a-reference.md`](guides/capture-a-reference.md),
  [`guides/replay-a-candidate.md`](guides/replay-a-candidate.md), and
  [`guides/validate-and-compare.md`](guides/validate-and-compare.md) — the
  capture, replay, and compare workflow.
- [`guides/async-capture.md`](guides/async-capture.md) — async operations in
  Rust and C#.
- [`guides/port-a-component.md`](guides/port-a-component.md) — compare a Rust
  implementation with a C# one.
- [`guides/troubleshoot-capture.md`](guides/troubleshoot-capture.md) — common
  diagnostics and fixes.

## Reference

- [`reference/cli.md`](reference/cli.md) — commands, options, defaults, exit
  codes, environment variables, and current output.
- [`reference/binding.md`](reference/binding.md) — `specgate.yaml` fields and
  their current behavior.
- [`reference/value-mapping.md`](reference/value-mapping.md) — how Rust and C#
  types map to CTSC values.
- [`reference/bundle.md`](reference/bundle.md) — files in a capture bundle.
- [`reference/limitations.md`](reference/limitations.md) — current limits by
  language and replay.
- [`status.md`](status.md) — architecture and current implementation status.

## Contracts (CTSC, normative)

- [`ctsc/README.md`](ctsc/README.md) — CTSC document set, corpus, and
  validation entry point.
- [`ctsc/trace.md`](ctsc/trace.md) — normative trace conventions.
- [`ctsc/registry.md`](ctsc/registry.md) — normative registry contract.
- [`ctsc/comparison.md`](ctsc/comparison.md) — comparison policy contract and
  CTSC Strict.

## Decisions

- [`decisions/`](decisions/README.md) — human-ratified architectural decisions.
- [`decisions/csharp-golden-replay-parity.md`](decisions/csharp-golden-replay-parity.md)
  — ratified single golden bundle for all languages (including canonical
  registry, now in case `expected/` directories), transient strict producer
  validation, and reference-to-candidate implementation checks.

## Contributor

- [`../CONTRIBUTING.md`](../CONTRIBUTING.md) — setup, CTSC-first development,
  generated artifacts, and the required gate.
- [`../AGENTS.md`](../AGENTS.md) — agent entry point and delivery boundaries.
- [`../.github/copilot-instructions.md`](../.github/copilot-instructions.md) —
  concise automatic coding instructions.
- [`../test/behavior/README.md`](../test/behavior/README.md) — behavior-first
  fixture sources, shared scenarios, aggregate projects, and generated
  ownership.
- [`contributor/agentic-loop.md`](contributor/agentic-loop.md) — observe → plan
  → act → verify workflow.
- [`contributor/roadmap.md`](contributor/roadmap.md) — tiered plan to MVP, V2,
  and V3, and the open questions gating each tier.
- [`contributor/dev-cycle-performance.md`](contributor/dev-cycle-performance.md)
  — gate timings, golden build cache internals, and smoke and workspace-test
  compilation-reuse measurements.
- [`contributor/component-identity-and-replay.md`](contributor/component-identity-and-replay.md)
  — exploratory notes on scenario identity, the replay unit, and artifact
  granularity. Not ratified.
- [`digests/llm.md`](digests/llm.md) — dense operational summary for agents.
- [`digests/human.md`](digests/human.md) — concise status, risks, and ownership
  boundaries.

### Agent customization layout

- `.github/agents/*.agent.md` defines the orchestrator and tool-scoped
  specialists selectable by GitHub Copilot.
- `.github/skills/*.md` contains reusable planning, implementation, and review
  playbooks applied by those specialists. Product usage lives in
  [`guides/`](guides/README.md); skills link there instead of repeating it.
- Specialist agents are not substitutes for repository evidence or human
  acceptance.

## Sources of truth

Use this precedence when documents appear to conflict:

1. The current human directive.
2. Ratified decision records and normative CTSC contracts.
3. Executable schemas, validators, tests, and hand-authored golden-matrix
   configuration.
4. Contributor and repository instructions.
5. Implementation-status documents.
6. Human and LLM digests.

Digests summarize durable facts; they do not create new requirements. Update
the authoritative document first, then refresh affected digests.
