# SpecGate agent guide

> **Audience: SpecGate developers, not SpecGate users.** This guide is for
> changing the SpecGate repository itself. To instrument, capture, replay, or
> compare another codebase with SpecGate, follow
> [`docs/guides/README.md`](docs/guides/README.md) instead.

This is the repository entry point for coding agents. It organizes the
authoritative project guidance; it does not replace the CTSC contracts or
contributor rules.

## Read before changing the repository

1. [`docs/README.md`](docs/README.md) — documentation map and precedence.
2. [`docs/digests/llm.md`](docs/digests/llm.md) — operational facts,
   invariants, limitations, and decision boundaries.
3. [`docs/status.md`](docs/status.md) and
   [`docs/reference/limitations.md`](docs/reference/limitations.md) — current
   implementation status and product limitations.
4. The relevant contract under [`docs/ctsc/`](docs/ctsc/) and the source and
   tests for the area being changed.

Also follow [`.github/copilot-instructions.md`](.github/copilot-instructions.md)
for the concise repository coding rules and
[`CONTRIBUTING.md`](CONTRIBUTING.md) for the contributor gate.

## Using SpecGate

To instrument, capture, replay, validate, or compare with SpecGate (rather than
change it), follow the task guides in [`docs/guides/`](docs/guides/README.md),
starting with the
[agent quick path](docs/guides/README.md#agent-quick-path). CLI, binding, and
bundle facts are in [`docs/reference/`](docs/README.md#reference).

## Delivery workflow

The fixture corpus starts at [`test/behavior/README.md`](test/behavior/README.md).
Cases own real sources and shared native bundles; aggregate projects preserve
batching and native test identities. Only declared case and rejection
`expected/` directories are generated; never clear a mixed case or test root.

Use the observe → plan → act → verify loop described in
[`docs/contributor/agentic-loop.md`](docs/contributor/agentic-loop.md).

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
