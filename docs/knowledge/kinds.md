# Operation kinds (historical)

> **Superseded.** The current model has no `kind` taxonomy. Every
> `#[spec_operation("…")]` in the fixtures is name-only; the shape of an
> operation is expressed entirely by what events the spec lists in
> `expected:`.
>
> The previous taxonomy
> (`Stateless` / `StateMachine` / `Sequence` / `ErrorMap` / `Structural`)
> and the per-kind role restrictions described in earlier revisions of
> this file have been removed.
>
> For the current annotation model, see:
> - [`annotations.md`](annotations.md) — the five annotations
> - [`spec-format.md`](spec-format.md) — case shape, `expected:`, subsequence matching
> - [`../design.md`](../design.md) — full design
> - Fixtures in `test/rust/crates/specgate-fixtures/` — canonical examples
