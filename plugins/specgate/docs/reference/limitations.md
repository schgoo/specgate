# Limitations

This page lists current product limits in one table. "Replay" means candidate
replay (`specgate replay`). For an explanation of how the pieces fit together,
see [`../status.md`](https://github.com/schgoo/specgate/blob/v0.6.0/docs/status.md).

| Limitation | Rust | C# | Replay | Tracking |
| --- | --- | --- | --- | --- |
| Capture runs a binding's `tests.command` form | Rejected | Rejected | n/a | [#46](https://github.com/schgoo/specgate/issues/46) |
| Rust capture reads `tests.projects` | Ignored; capture builds and runs the package at `sources` | Required | n/a | [#96](https://github.com/schgoo/specgate/issues/96) |
| Replay engine selection | Capture uses the original tests' configuration | n/a | Arty, Smol, or Tokio from candidate dependencies; ambiguous selection requires `rust.runtime`. Custom configuration uses `rust.executor` | [binding](binding.md) |
| Parallel tests within one capture process | Each test runs in its own process with `--test-threads=1`; a correctness constraint | Each test runs in its own `dotnet test` process | n/a | [#70](https://github.com/schgoo/specgate/issues/70) |
| Candidate language | n/a | n/a | Rust candidates only (`unsupported-language`) | [status](https://github.com/schgoo/specgate/blob/v0.6.0/docs/status.md#components) |
| Operation shape | n/a | n/a | Public free functions and setup-backed instance methods borrowing `&self` or `&mut self`, including directly awaited async calls and explicit generic specializations. Consuming receivers are rejected during planning | [typed async replay](https://github.com/schgoo/specgate/blob/v0.6.0/docs/decisions/typed-async-client-replay.md) |
| Setup-backed operations | Captured | Captured | Annotated synchronous nongeneric producers reconstruct receivers and injected parameters; private/test-only producers run in one selected Cargo test target | [typed async replay](https://github.com/schgoo/specgate/blob/v0.6.0/docs/decisions/typed-async-client-replay.md) |
| Generated discovery/replay hooks | Each Cargo target needs a crate-root `spec_component!`; a library declaration does not cover binary or integration-test crates | n/a | The selected Rust test target must declare its own component, even for explicitly component-qualified operations | [Rust instrumentation](../guides/instrument-rust-crate.md) |
| Structured input and output values | Captured | Captured | Inputs support lossless primitives and recursively supported lists, sets, string-keyed maps, and optionals. Explicit property setups recursively decompose nested opted-in records, including producer arguments, into flat semantic leaves; omitted Rust fields use `Default`. Duplicate names and cyclic expansions are rejected. Records inside containers and general record inputs remain unsupported (`structured-value`). Structured results/errors require matching reachable CTSC declarations | [constructor and property setups](https://github.com/schgoo/specgate/blob/v0.6.0/docs/decisions/constructor-and-property-setups.md) |
| Nested operations | Captured | Captured | Only top-level reference operations are invoked; nested ones are compared as observed behavior | — |
| Explicit reference-to-candidate mappings | n/a | n/a | Not implemented | — |
| Async operations | Captured (directly awaited `async fn`) | Captured (`Task`, `Task<T>`, `ValueTask`, `ValueTask<T>`) | Directly awaited calls use the candidate's selected engine or custom wrapper; overlapping sibling scheduling is unsupported | [async-capture-context](https://github.com/schgoo/specgate/blob/v0.6.0/docs/decisions/async-capture-context.md), [csharp-async-capture](https://github.com/schgoo/specgate/blob/v0.6.0/docs/decisions/csharp-async-capture.md) |
| Async setups | Component rejected before capture | Rejected by the weaver | n/a | [async-capture-context](https://github.com/schgoo/specgate/blob/v0.6.0/docs/decisions/async-capture-context.md) |
| `const fn` operations and setups | Compile error; annotate a non-const wrapper | n/a | n/a | [Rust guide](../guides/instrument-rust-crate.md#8-const-fn-operations) |
| Other async shapes | Operations first reached on a raw `std::thread::spawn` thread are not covered | `async void`, `async` methods returning other awaitables, iterators, and async iterators are rejected by the weaver | n/a | [csharp-async-capture](https://github.com/schgoo/specgate/blob/v0.6.0/docs/decisions/csharp-async-capture.md) |
| Interleaved sibling operations | Captured | Captured | Unreplayable: strict comparison rejects overlapping sequential children | [async-capture-context](https://github.com/schgoo/specgate/blob/v0.6.0/docs/decisions/async-capture-context.md) |
| Top-level abandonment (an abandoned operation with no enclosing and no later annotated operation) | Scenario dropped and reported as a failed test; enclose it in a completing operation instead | Not a limitation: cancellation is persisted immediately, so a passing test keeps its scenario. A task still outstanding when the session finishes is recorded as a fault and that test contributes no scenario | n/a | [top-level-abandonment-unsupported](https://github.com/schgoo/specgate/blob/v0.6.0/docs/decisions/top-level-abandonment-unsupported.md) |
| Observations (`spec_trace!`) | Captured but never declared in the registry, so the bundle cannot link | No public observation API | n/a | — |
| Generic operations | Explicit repeatable `types` specializations; Rust substitutions must preserve syntactic completion channels | Explicit repeatable `TypeArguments` specializations; discovery validates CLR constraints, not compiler-only rules. Open generic declaring types are rejected | Concrete specializations only; generic setups and Rust const generics are unsupported | [typed async replay](https://github.com/schgoo/specgate/blob/v0.6.0/docs/decisions/typed-async-client-replay.md) |
| By-reference or pointer parameters | Rust references can receive annotated setup outputs | Rejected by the weaver | C# candidate replay is unsupported | — |
| Unsigned integers | Supported for `u32`, `u64` | Supported for `uint`, `ulong` | Supported for `u32`, `u64` | [value mapping](value-mapping.md) |
| Discovery rejections | Duplicate operation identity, orphan setups, method operations without a receiver setup, operations on private functions, and the dynamic runtime `Value` are rejected, because none can describe a well-formed CTSC component surface | See the C# rows above and [value mapping](value-mapping.md) | n/a | — |
| Comparison policies | n/a | n/a | Only the fixed `ctsc.strict/0.1.0`; multiple-run selection, overlapping sequential children, and duplicate parallel branch identities are rejected | [`../ctsc/comparison.md`](https://github.com/schgoo/specgate/blob/v0.6.0/docs/ctsc/comparison.md) |
| Abandonment in comparison | n/a | n/a | No special handling: an abandoned operation differs from a completed one only through event-name matching | [abandonment-terminal-state](https://github.com/schgoo/specgate/blob/v0.6.0/docs/decisions/abandonment-terminal-state.md) |
| Concurrent capture sessions in one process | Per-run collector; raw-thread-first activation unsupported | A second concurrent session is rejected | n/a | — |

Codes in parentheses are the stable replay limitation categories reported by
the golden matrix (`expectCategory`).

Native validation supports JSON and JSONL traces, registry imports, exact
capture-bundle integrity, and linked type checking. Bundle validation is
intentionally independent from replay's narrower invocation decoder.
