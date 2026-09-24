# SpecGate development loop

## Purpose

Use a controller to shorten research, implementation, and verification cycles
without moving architecture or acceptance authority away from humans. Human
review remains the final gate.

## Roles

| Role | Tool posture | Primary output |
|---|---|---|
| Orchestrator | Observe, delegate, validate | Controlled delivery loop |
| Planner | Read-only | Bounded task packet and stop conditions |
| Implementer | Read, edit, validate | Code, tests, artifacts, and evidence |
| Reviewer | Read-only except validation | Blocking findings or review pass |

The custom agent profiles are under `.github/agents/`; reusable playbooks are
under `.github/skills/`.

## Control loop

1. **Observe** — reconcile the user directive, worktree, relevant CTSC
   contracts, migration status, source, tests, and latest validation evidence.
   Treat summaries and prior agent claims as unverified until supported by
   repository evidence. Inspect repository-controlled executable changes before
   running them.
2. **Plan** — choose one bounded vertical slice. Name the observable outcome,
   exact files and responsibilities in scope, prerequisites, behavior evidence,
   validation commands, boundaries that must not change, and human stop
   conditions.
3. **Act** — execute the approved packet without broadening it. Only one
   mutating agent runs at a time. Preserve unrelated changes and use existing
   CTSC and repository mechanisms rather than parallel assertion formats.
4. **Verify** — inspect the actual diff, not the implementer's summary.
   Regenerate intended golden artifacts, review their diff, run focused checks,
   and finish with `just check`. Run `just package-smoke` for release or
   packaging changes.
5. **Stop or repeat** — stop at completion, an unresolved human-owned decision,
   an external blocker, or a repeated failure that requires replanning.

## Loop guards

- Do not repeat an unchanged action after it fails; re-observe and revise.
- Do not broaden product scope merely to make validation pass.
- Do not hand-edit generated files under `test/goldens/ctsc`.
- Do not silently finalize a decision listed in `AGENTS.md`.
- Do not commit, push, open or merge pull requests, or otherwise mutate remote
  state without explicit authorization.
- Generated artifact volume does not justify an oversized hand-written change.
  Bound slices by responsibility and reviewability rather than a mechanical
  line limit.

## Completion handoff

Return the delivered behavior or documentation, important files changed,
validation evidence, open assumptions, and the exact human action required if
blocked.
