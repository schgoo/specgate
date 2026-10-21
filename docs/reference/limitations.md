# Limitations

This page lists current product limits in one table. "Replay" means candidate
replay (`specgate replay`). For an explanation of how the pieces fit together,
see [`../status.md`](../status.md).

| Limitation | Rust | C# | Replay | Tracking |
| --- | --- | --- | --- | --- |
| Capture runs a binding's `tests.command` form | Rejected | Rejected | n/a | [#46](https://github.com/schgoo/specgate/issues/46) |
| Rust capture reads `tests.projects` | Ignored; capture builds and runs the package at `sources` | Required | n/a | [#96](https://github.com/schgoo/specgate/issues/96) |
| `rust.runtime` binding field | Parsed, not used by the CLI | n/a | n/a | [#95](https://github.com/schgoo/specgate/issues/95) |
| Parallel tests within one capture process | Each test runs in its own process with `--test-threads=1`; a correctness constraint | Each test runs in its own `dotnet test` process | n/a | [#70](https://github.com/schgoo/specgate/issues/70) |
| Candidate language | n/a | n/a | Rust candidates only (`unsupported-language`) | [status](../status.md#components) |
| Operation shape | n/a | n/a | Synchronous public free functions only (`async-operation`, `method-operation`) | [async-capture-context](../decisions/async-capture-context.md) |
| Setup-backed operations | Captured | Captured | Not constructed (`setup-backed-operation`) | — |
| Structured input and output values | Captured | Captured | Lossless primitives only: unit, string, bool, `i32`, `i64`, `u32`, `u64`, `f32`, `f64` (`structured-value`) | — |
| Nested operations | Captured | Captured | Only top-level reference operations are invoked; nested ones are compared as observed behavior | — |
| Explicit reference-to-candidate mappings | n/a | n/a | Not implemented | — |
| Async operations | Captured (directly awaited `async fn`) | Captured (`Task`, `Task<T>`, `ValueTask`, `ValueTask<T>`) | Not replayed (`async-operation`) | [async-capture-context](../decisions/async-capture-context.md), [csharp-async-capture](../decisions/csharp-async-capture.md) |
| Async setups | Component rejected before capture | Rejected by the weaver | n/a | [async-capture-context](../decisions/async-capture-context.md) |
| Other async shapes | Operations first reached on a raw `std::thread::spawn` thread are not covered | `async void`, `async` methods returning other awaitables, iterators, and async iterators are rejected by the weaver | n/a | [csharp-async-capture](../decisions/csharp-async-capture.md) |
| Interleaved sibling operations | Captured | Captured | Unreplayable: strict comparison rejects overlapping sequential children | [async-capture-context](../decisions/async-capture-context.md) |
| Top-level abandonment (an abandoned operation with no enclosing and no later annotated operation) | Scenario dropped and reported as a failed test; enclose it in a completing operation instead | Not a limitation: cancellation is persisted immediately, so a passing test keeps its scenario. A task still outstanding when the session finishes is recorded as a fault and that test contributes no scenario | n/a | [top-level-abandonment-unsupported](../decisions/top-level-abandonment-unsupported.md) |
| Observations (`spec_trace!`) | Captured but never declared in the registry, so the bundle cannot link | No public observation API | n/a | — |
| Generic methods or types, by-reference or pointer parameters | n/a | Rejected by the weaver | n/a | — |
| Unsigned integers | Supported for `u32`, `u64` | Supported for `uint`, `ulong` | Supported for `u32`, `u64` | [value mapping](value-mapping.md) |
| Discovery rejections | Duplicate operation identity, orphan setups, method operations without a receiver setup, operations on private functions, and the dynamic runtime `Value` are rejected, because none can describe a well-formed CTSC component surface | See the C# rows above and [value mapping](value-mapping.md) | n/a | — |
| Comparison policies | n/a | n/a | Only the fixed `ctsc.strict/0.1.0`; multiple-run selection, overlapping sequential children, and duplicate parallel branch identities are rejected | [`../ctsc/comparison.md`](../ctsc/comparison.md) |
| Abandonment in comparison | n/a | n/a | No special handling: an abandoned operation differs from a completed one only through event-name matching | [abandonment-terminal-state](../decisions/abandonment-terminal-state.md) |
| Concurrent capture sessions in one process | Per-run collector; raw-thread-first activation unsupported | A second concurrent session is rejected | n/a | — |

Codes in parentheses are the stable replay limitation categories reported by
the golden matrix (`expectCategory`).

Native validation supports JSON and JSONL traces, registry imports, exact
capture-bundle integrity, and linked type checking. Bundle validation is
intentionally independent from replay's narrower invocation decoder.
