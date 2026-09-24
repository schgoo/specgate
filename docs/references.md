# SpecGate references

## Repository guidance

- [`../AGENTS.md`](../AGENTS.md) — agent entry point.
- [`README.md`](README.md) — documentation map and source-of-truth precedence.
- [`agentic-loop.md`](agentic-loop.md) — delivery controller workflow.
- [`../CONTRIBUTING.md`](../CONTRIBUTING.md) — contributor setup and gate.
- [`../.github/copilot-instructions.md`](../.github/copilot-instructions.md) —
  concise automatic coding instructions.

## Product contracts

- [`ctsc/README.md`](ctsc/README.md) — CTSC document and corpus index.
- [`ctsc/trace.md`](ctsc/trace.md) — trace contract.
- [`ctsc/registry.md`](ctsc/registry.md) — registry contract.
- [`ctsc/comparison.md`](ctsc/comparison.md) — comparison policy contract.
- [`specgate-ctsc-migration.md`](specgate-ctsc-migration.md) — implemented
  architecture and current limitations.

## Agent customization layout

- `.github/agents/*.agent.md` defines the orchestrator and tool-scoped
  specialists selectable by GitHub Copilot.
- `.github/skills/*.md` contains reusable planning, implementation, and review
  playbooks applied by those specialists.
- Specialist agents are not substitutes for repository evidence or human
  acceptance.
