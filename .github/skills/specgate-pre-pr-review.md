# Skill: specgate-pre-pr-review

Independently verify a SpecGate slice before human review. Read-only except for
running validation.

## Steps

1. Inspect the actual diff. Before executing repository-controlled code,
   inspect changes to manifests, lockfiles, build scripts, proc-macros, task
   runners, and validation scripts.
2. Check the approved packet and relevant CTSC contracts:
   - scope and responsibility boundaries are respected;
   - tests cover the changed behavior through existing CTSC mechanisms;
   - runtime dependency identity remains exact;
   - new annotated fixtures have complete matrix coverage;
   - parity differences, capture exclusions, replay categories, linkage, and
     build-negative source attribution remain exact;
   - generated artifacts came from `just ctsc-goldens-update` and their diff is
     intentional;
   - crate README changes originate from crate-level docs;
   - no human-owned decision was silently finalized.
3. Run focused validation and the complete gate:

   ```text
   just check
   ```

4. Run `just package-smoke` when release or packaging behavior changed.

## Output

Return blocking findings with exact acceptance criteria, or a pass confirming
the slice is ready for human review. Do not edit files.
