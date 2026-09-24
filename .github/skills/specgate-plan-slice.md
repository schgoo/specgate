# Skill: specgate-plan-slice

Turn one SpecGate objective into a bounded, evidence-backed task packet.
Read-only.

## Steps

1. Read the user directive, `AGENTS.md`, `docs/digests/llm.md`,
   `docs/specgate-ctsc-migration.md`, the relevant CTSC contracts, and affected
   source and tests.
2. Confirm prerequisites. Stop if required implementation, toolchain, fixture,
   or human decision is missing.
3. Define:
   - the observable outcome;
   - exact files, crates, and responsibilities in scope;
   - boundaries that must not change;
   - focused registry, trace, capture, replay, validator, or comparator
     evidence;
   - matrix rows or generated artifacts expected to change;
   - focused validation recipes and the final `just check`;
   - release/package validation when applicable;
   - human stop conditions.
4. Surface human-owned decisions listed in `AGENTS.md`. Do not resolve them.

## Output

A task packet the implementer can execute without re-deriving scope. Do not
edit files.
