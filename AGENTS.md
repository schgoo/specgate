# SpecGate agent guide

This is the repository entry point for coding agents. It organizes the
authoritative project guidance; it does not replace the CTSC contracts or
contributor rules.

## Read before changing the repository

1. [`docs/README.md`](docs/README.md) — documentation map and precedence.
2. [`docs/digests/llm.md`](docs/digests/llm.md) — operational facts,
   invariants, limitations, and decision boundaries.
3. [`docs/specgate-ctsc-migration.md`](docs/specgate-ctsc-migration.md) —
   current implementation status and product limitations.
4. The relevant contract under [`docs/ctsc/`](docs/ctsc/) and the source and
   tests for the area being changed.

Also follow [`.github/copilot-instructions.md`](.github/copilot-instructions.md)
for the concise repository coding rules and
[`CONTRIBUTING.md`](CONTRIBUTING.md) for the contributor gate.

## Delivery workflow

Use the observe → plan → act → verify loop described in
[`docs/agentic-loop.md`](docs/agentic-loop.md).

- Plan one bounded vertical slice with explicit file and responsibility
  boundaries.
- Preserve unrelated worktree changes.
- Make behavior changes CTSC-first: add focused registry, trace, capture,
  replay, validator, or comparator evidence through the existing test
  mechanisms.
- Regenerate generated CTSC goldens with `just ctsc-goldens-update`; never edit
  them manually.
- Run focused recipes while iterating and `just check` from the repository root
  before commit or handoff. Release and packaging changes also run
  `just package-smoke`.

Custom delivery roles live under [`.github/agents/`](.github/agents/) and their
reusable playbooks live under [`.github/skills/`](.github/skills/).

## Human-owned decisions

Stop and request an explicit owner before silently changing:

- normative CTSC document shapes or validation semantics;
- the public CLI command or argument surface;
- capture and replay semantics, including operation identity or stimulus
  selection;
- comparison policy semantics;
- runtime package-identity and generated-runner dependency rules;
- the meaning of golden-matrix parity, linkage, coverage, or replacement
  declarations.

Record ratified architectural decisions under [`docs/decisions/`](docs/decisions/).
Human review remains the acceptance gate.
