# Changelog

## [Unreleased — 0.6.0]

### Changed

- `#[spec_operation]` now instruments an `async fn` instead of rejecting it.
  Recording begins at the future's first poll, not at construction, and the
  `OperationScope` is held across the body's awaits, so a directly-awaited
  operation records its inputs, result, and nesting exactly like a synchronous
  call. The function signature is untouched, so discovery metadata — including
  `return_type` — is byte-for-byte unchanged and parity with the C# twin holds.
  The sync and async expansions now share one per-return-kind completion
  fragment, so the two paths cannot drift. Async `#[spec_setup]` stays
  uninstrumented and its component stays discovery-only, because deferred setup
  inputs are still staged outside the per-run collector. Out of scope and
  failing closed: futures that migrate between executor threads, futures
  abandoned while still `Pending`, and futures polled concurrently with another
  instrumented operation (the active-operation stack is a single nesting
  stack). `reject_async_native_capture` is retained but no longer called.
- A regression test pins that two instrumented operations polled concurrently on
  a single thread fail closed. `active_operations` is a single nesting stack, so
  the first future to finish is no longer on top of it and completion panics
  rather than recording a wrong trace. The test exists so a future change cannot
  silently turn that loud failure into a bad bundle.
- `fixture.async_fetch` graduates from a discovery-only golden row to a full
  capture row with a linked bundle. Replay stays unsupported for it with the
  `async-operation` category, because the replay planner emits only synchronous
  candidate calls.
- `fixture.extract`, `fixture.async_smol_timer`, and `fixture.async_tokio_timer`
  gain fixture tests that drive their async operations and graduate from
  discovery-only golden rows to full capture rows with linked bundles. The timer
  fixtures are driven by a bare `smol::block_on` and a current-thread Tokio
  runtime, so neither future is migrated between threads. No matrix row carries
  the `async-capture-unsupported` limitation any more. Replay stays unsupported
  for all three: `async-operation` for the two timer rows, and
  `structured-value` for `fixture.extract`, whose `find` operation takes a
  `Vec<i32>` and is rejected before planning reaches the async `fetch`.
- `fixture.fallible_unit` also gains a test that awaits its async
  `fallible_task` operation on both the `Ok` and `Err` paths, but the row stays
  discovery-only. Its blocker is no longer async: `UnitCounter::advance` emits a
  `spec_trace!` observation that discovery cannot declare, so a bundle fails
  validation with `observation 'count' is not declared`. Its limitation is
  restated as `observation-not-declared`.
- Native capture state is now owned by a per-run collector rather than by a
  thread-local. `OperationScope` clones a shared, `Send` handle to its recording
  at construction and works from that handle instead of re-reading ambient
  storage. The ambient slot stays thread-local, so test isolation is unchanged;
  public signatures, generated macro output, generated runner source, and every
  golden artifact are unchanged. This is the precondition for carrying capture
  context into a future. Mutex poisoning is recovered deterministically, so a
  capture whose critical section panicked still produces its
  `incomplete_capture` fault or terminal error rather than an opaque lock error.
- Trace validation now rejects a `conformance.operation` span that carries no
  completion or failure event unless its status is `OK`. Trace §7.5 requires an
  unfinished operation to carry an `incomplete_capture` fault, so a span with no
  terminal event is a §7.6 unit completion and any other status is
  self-contradictory. Previously an abandoned operation validated clean. The
  rule is enforced identically by the native validator and `docs/ctsc/validate.py`.
- Registry documents now order their `components` array by ascending component
  id in every encoder. The selected component no longer leads its own
  dependencies, so `discover` and `capture` emit byte-identical bytes for the
  same component set. `registryId` is unchanged.
- Component capture no longer rejects a scenario that calls into other
  annotated components. It now exports the selected component's top-level
  operation subtrees verbatim, keeping nested foreign operations with their
  original parentage, and emits a registry that is the union of every component
  present in the filtered trace.
- `finish_native_capture` now closes every still-outstanding operation with the
  core target fault `incomplete_capture` (observer `target`) on that operation's
  own span, innermost first, and persists the sidecar before returning the
  error that names the outstanding chain. Trace §7.5 required the fault; no
  emission site existed.
- The native capture sidecar is now a continuously valid provisional snapshot,
  rewritten once each operation's declared inputs are recorded and again at
  every operation close, rather than only when the operation stack empties.
  `specgate capture` has no end-of-test signal, so a leaked operation scope
  previously left no sidecar at all and the run still reported success; the
  operation now appears with an `incomplete_capture` target fault in a bundle
  that still passes trace, linked, and bundle validation. While operations are
  outstanding the snapshot is projected from a clone, so the live logical clock
  is untouched and a completed recording is byte-identical to before.
- `#[spec_operation]` now emits `OperationScope::inputs_recorded` after
  recording an operation's inputs and before running its body, which is the
  provisional snapshot site.
- Replaced the legacy verification stack with CTSC-native discovery, capture,
  and replay.
- Added the focused `specgate-discovery` crate and one strict target-binding
  resolver shared by all CLI commands.
- Reduced Rust and C# fixtures to stateless, rich-type, and setup-discovery
  coverage.

### Removed

- Spec-driven validation, execution, matching, extraction, code generation,
  coverage, and self-hosting.
- Flat trace recording and table-driven mock instrumentation.
- The former harness/types crates and C# runtime/weaver projects.
- The standalone Rust `specgate-annotations` facade; `specgate` is the sole
  macro/runtime facade.
