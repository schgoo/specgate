# Limitations

This page lists current product limits. "Replay" means candidate
replay (`specgate replay`). For an explanation of how the pieces fit together,
see [`../status.md`](../status.md).

| Limitation | Rust | C# | Replay | Tracking |
| --- | --- | --- | --- | --- |
| Capture runs a binding's `tests.command` form | Rejected | Rejected | n/a | [#46](https://github.com/schgoo/specgate/issues/46) |
| Rust capture reads `tests.projects` | Ignored; capture builds and runs the package at `sources` | Required | n/a | [#96](https://github.com/schgoo/specgate/issues/96) |
| Replay engine selection | Capture uses the original tests' configuration | n/a | Arty, Smol, or Tokio from candidate dependencies; ambiguous selection requires `rust.runtime`. Custom configuration uses `rust.executor` | [binding](binding.md) |
| Parallel tests within one capture process | Each test runs in its own process with `--test-threads=1`; a correctness constraint | Each test runs in its own `dotnet test` process | n/a | [#70](https://github.com/schgoo/specgate/issues/70) |
| Candidate language | n/a | n/a | Rust candidates only (`unsupported-language`) | [status](../status.md#components) |
| Operation shape | n/a | n/a | Public free functions and setup-backed instance methods borrowing `&self` or `&mut self`, including directly awaited async calls and explicit generic specializations. Consuming receivers are rejected during planning | [typed async replay](../decisions/typed-async-client-replay.md) |
| Setup-backed operations | Captured | Captured | Annotated synchronous nongeneric producers reconstruct receivers and injected parameters; private/test-only producers run in one selected Cargo test target | [typed async replay](../decisions/typed-async-client-replay.md) |
| Generated discovery/replay hooks | Each Cargo target needs a crate-root `spec_component!`; a library declaration does not cover binary or integration-test crates | n/a | The selected Rust test target must declare its own component, even for explicitly component-qualified operations | [Rust instrumentation](../guides/instrument-rust-crate.md) |
| Structured input and output values | Captured | Captured | Inputs support lossless primitives, tuples of arity 2-4, and recursive collections, supported map keys, and optionals. Property setups fold opted-in records into flat leaves; omitted Rust fields use `Default`. Duplicate names and cyclic expansions reject. General record inputs, including records in containers, remain unsupported (`structured-value`). Results/errors use checked nested storage bindings with named identity retained. Inline reference records can bind to same-component annotated Rust records with exact fields; the reverse direction rejects | [value mapping](value-mapping.md), [constructor and property setups](../decisions/constructor-and-property-setups.md) |
| Cross-type input conversions | n/a | n/a | Current replay rejects unsupported input mappings; use a user-authored wrapper. Automatic Rust `From` conversions are approved but not implemented. No conversion-binding or converter-registration API; existing checked storage bindings are unchanged | [input compatibility](#input-type-compatibility) |
| Nested operations | Captured | Captured | Only top-level reference operations are invoked and require candidate declarations. Missing observed-only calls are comparison differences, not planning errors. Declared nested calls bind in their owning component, including other components. Extra calls use checked storage conversions against the reference registry, including pair-vector maps. Incompatible unused declarations reject only if captured; undeclared or ill-typed calls fail before publication | — |
| Explicit reference-to-candidate mappings | n/a | n/a | Not implemented | — |
| Inherited record members | n/a | Supported base-first for ordinary records, property setups, and union payloads. Overrides and `new` replace by source name in the original slot before annotation selection; semantic-name collisions across distinct members reject. Methods remain declared-only | No new C# candidate replay support | #127 delivered; [value mapping](value-mapping.md#c-inherited-record-members) |
| Rust type aliases at annotated boundaries | Automatic compiler-directed alias resolution is not implemented | n/a | No new alias bindings | [#122](https://github.com/schgoo/specgate/issues/122) |
| Ignored context | Operation and synchronous setup-producer parameters support `#[spec_input(ignore)]`, requiring `Default` without projection; `#[spec_event(ignore)]` omits record and named/tuple enum payload fields before mapping. Ignored-only setup annotations select zero fields | By-value operation and setup-producer parameters support `SpecInput.Ignore`; record fields/properties support `SpecEvent.Ignore`, including nested records, inherited members, union payloads, and property setups. Any effective member annotation, even ignored-only, disables setup all-member inference | Rust defaults ignored arguments. For ignored fields, producers own construction; property setups prefer whole-struct `Default`, then field defaults if unavailable. Other unannotated omissions still require whole-struct default. No C# candidate support | [value mapping](value-mapping.md#ignored-context-inputs), [reconstruction precedence](../decisions/constructor-and-property-setups.md#ignored-field-reconstruction-owner-approved-2026-11-17) |
| Collection dimensions | Native vectors/slices are ordered nonunique; sets/maps retain unordered defaults | Generic collection contracts, keyed lookups/groupings, ordered dictionaries, and priority element/priority pairs are captured | `Vec` stores lists, bags, sets, and ordered sets. Pair vectors store ordered and non-string maps; native maps remain unordered and string-keyed. Bags keep existing input sorting. Custom semantic overrides and arbitrary native map keys are not implemented | [storage bindings](../decisions/registry-guided-collection-replay.md) |
| Erased collections and cursors | No new mapping | Non-generic `ArrayList`/`Hashtable`, caller-owned enumerators, async streams, and non-enumerable buffers have no automatic mapping | No contents-based schema inference or cursor lifecycle model | [value mapping](value-mapping.md) |
| Async operations | Captured (directly awaited `async fn`) | Captured (`Task`, `Task<T>`, `ValueTask`, `ValueTask<T>`) | Directly awaited calls use the candidate's selected engine or custom wrapper; overlapping sibling scheduling is unsupported | [async-capture-context](../decisions/async-capture-context.md), [csharp-async-capture](../decisions/csharp-async-capture.md) |
| Async setups | Component rejected before capture | Rejected by the weaver | n/a | [async-capture-context](../decisions/async-capture-context.md) |
| `const fn` operations and setups | Compile error; annotate a non-const wrapper | n/a | n/a | [Rust guide](../guides/instrument-rust-crate.md#8-const-fn-operations) |
| Other async shapes | Operations first reached on a raw `std::thread::spawn` thread are not covered | `async void`, `async` methods returning other awaitables, iterators, and async iterators are rejected by the weaver | n/a | [csharp-async-capture](../decisions/csharp-async-capture.md) |
| Interleaved sibling operations | Captured | Captured | Unreplayable: strict comparison rejects overlapping sequential children | [async-capture-context](../decisions/async-capture-context.md) |
| Top-level abandonment (an abandoned operation with no enclosing and no later annotated operation) | Scenario dropped and reported as a failed test; enclose it in a completing operation instead | Not a limitation: cancellation is persisted immediately, so a passing test keeps its scenario. A task still outstanding when the session finishes is recorded as a fault and that test contributes no scenario | n/a | [top-level-abandonment-unsupported](../decisions/top-level-abandonment-unsupported.md) |
| Observations (`spec_trace!`) | Captured but never declared in the registry, so the bundle cannot link | No public observation API | n/a | — |
| Generic operations | Explicit repeatable `types` specializations; Rust substitutions must preserve syntactic completion channels | Explicit repeatable `TypeArguments` specializations; discovery validates CLR constraints, not compiler-only rules. Open generic declaring types are rejected | Concrete specializations only; generic setups and Rust const generics are unsupported | [typed async replay](../decisions/typed-async-client-replay.md) |
| By-reference, pointer, or non-boxable parameters | Rust references can receive annotated setup outputs | Rejected by the weaver, including ignored ref structs and generic parameters with `allows ref struct` | C# candidate replay is unsupported | — |
| Unsigned integers | Supported for `u32`, `u64` | Supported for `uint`, `ulong` | Supported for `u32`, `u64` | [value mapping](value-mapping.md) |
| Discovery rejections | Duplicate operation identity, orphan setups, method operations without a receiver setup, operations on private functions, and the dynamic runtime `Value` are rejected, because none can describe a well-formed CTSC component surface | See the C# rows above and [value mapping](value-mapping.md) | n/a | — |
| Comparison policies | n/a | n/a | Only the fixed `ctsc.strict/0.2.0`; multiple-run selection, overlapping sequential children, and duplicate parallel branch identities are rejected | [`../ctsc/comparison.md`](../ctsc/comparison.md) |
| Abandonment in comparison | n/a | n/a | No special handling: an abandoned operation differs from a completed one only through event-name matching | [abandonment-terminal-state](../decisions/abandonment-terminal-state.md) |
| Concurrent capture sessions in one process | Per-run collector; raw-thread-first activation unsupported | A second concurrent session is rejected | n/a | — |

Codes in parentheses are the stable replay limitation categories reported by
the golden matrix (`expectCategory`).

Native validation supports JSON and JSONL traces, registry imports, exact
capture-bundle integrity, and linked type checking. Bundle validation is
intentionally independent from replay's narrower invocation decoder.

## Input type compatibility

**Current behavior:** replay does not convert an optional integer to a required
integer or a floating point input to an integer. It rejects these incompatible
input declarations even when the recorded value is `Some(4326)` or `4326.0`.
It does not infer a conversion from observed values, round or truncate numbers,
or replace `None` with a default.

If the native candidate takes different input types, write a wrapper that
accepts compatible recorded input types and implements the intended conversion,
default, or error policy in ordinary code. Annotate and test that wrapper as the
candidate boundary. Comparison then assesses the wrapped implementation, not
equivalence of the unwrapped native APIs. A wrapper failure follows ordinary
execution rules; it is not a request to skip the scenario.

Existing supported primitive and collection storage bindings, setup producers,
and explicitly opted-in default rules retain their current behavior. This
restriction concerns replay inputs, not output comparison.

### Approved Rust direction (not implemented)

Owner-approved on 2026-11-20: for a constructible recorded input `a: A` and a
candidate input type `B`, automatically accept and use the conversion when the
Rust compiler confirms `B: From<A>`. No separate converter registration or
per-mapping opt-in is required. This refers to the standard `From` trait, not
an arbitrary inherent method named `from`.

Illustrative generated call; this helper is not a new public API:

```rust
fn convert_input<A, B: From<A>>(recorded: A) -> B {
    <B as From<A>>::from(recorded)
}
```

For example, Rust provides `From<i32> for i64`, but not
`From<Option<i32>> for i32` or `From<f64> for i32`. User-defined implementations
for eligible types also qualify; their authors own the conversion behavior.
The trait's presence is the acceptance rule, not proof that custom code
preserves information or meaning.

If the source value cannot be constructed or the required implementation is
absent, the unsupported mapping remains rejected; users can provide wrappers.
Do not infer casts, rounding, truncation, or `None` defaults, or automatically
fall back to `TryFrom`. Conversion failures follow ordinary execution rules,
not scenario skipping. Conversion permits input construction; it does not
erase native type or signature differences. This direction is separate from
output comparison in ADR-0025.
