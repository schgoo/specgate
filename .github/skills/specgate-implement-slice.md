# Skill: specgate-implement-slice

Implement one approved SpecGate slice end to end.

## Rules

- Stay within the declared file and responsibility boundaries. Preserve
  unrelated changes.
- Make behavior changes CTSC-first through focused existing tests. Do not add a
  parallel assertion language or flat trace compatibility path.
- Reuse existing binding, capture, replay, validation, comparison, and
  golden-matrix mechanisms.
- Maintain exact runtime `PackageId` resolution; path and version overrides are
  assertions, not substitutions.
- Add a matrix row for every new annotated fixture component or source.
- Never hand-edit generated files in case and rejection `expected/` directories. Run
  `just ctsc-goldens-update` and review the artifact diff.
- Edit crate-level `lib.rs` documentation, then run `just readme`, rather than
  editing generated crate READMEs.
- When user-visible CLI, binding, capture, or replay behavior changes, update
  the affected page under `docs/guides/` or `docs/reference/`; `just docs-check`
  keeps the usage snapshot, links, binding examples, and quoted diagnostics in
  sync.
- Use `ohno` for new Rust error types.
- Add only necessary comments and keep LF line endings except for `.ps1`,
  `.cmd`, and `.bat`.

## Validate

Run the smallest focused recipes that cover the change while iterating. Before
handoff, run:

```text
just check
```

For release or packaging changes, also run:

```text
just package-smoke
```

Fix failures at their cause. Do not weaken tests, matrix declarations, lints,
or validators merely to pass.

## Output

Return the behavior delivered, important files and artifacts changed,
validation evidence, and open assumptions. Do not commit or push without
explicit authorization.
