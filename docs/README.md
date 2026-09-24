# SpecGate documentation

This index is the starting point for humans and coding agents.

## Product and contributor entry points

- [`../README.md`](../README.md) — product overview, commands, repository map,
  and golden-matrix summary.
- [`../CONTRIBUTING.md`](../CONTRIBUTING.md) — setup, CTSC-first development,
  generated artifacts, and the required gate.
- [`../AGENTS.md`](../AGENTS.md) — agent entry point and delivery boundaries.
- [`agentic-loop.md`](agentic-loop.md) — observe → plan → act → verify workflow.

## Contracts and status

- [`ctsc/README.md`](ctsc/README.md) — CTSC document set, corpus, and
  validation entry point.
- [`ctsc/trace.md`](ctsc/trace.md) — normative trace conventions.
- [`ctsc/registry.md`](ctsc/registry.md) — normative registry contract.
- [`ctsc/comparison.md`](ctsc/comparison.md) — comparison policy contract and
  CTSC Strict.
- [`specgate-ctsc-migration.md`](specgate-ctsc-migration.md) — implemented
  architecture, migration status, and current limitations.
- [`decisions/`](decisions/) — human-ratified architectural decisions.

## Digests and references

- [`digests/llm.md`](digests/llm.md) — dense operational summary for agents.
- [`digests/human.md`](digests/human.md) — concise status, risks, and ownership
  boundaries.
- [`references.md`](references.md) — repository guidance and tooling
  references.

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
