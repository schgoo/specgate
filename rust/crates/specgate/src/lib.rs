//! # `SpecGate`
//!
//! Deterministic spec-based verification for LLM-implemented code.
//!
//! Engineers write specs. LLMs implement them. `SpecGate` closes the gap by
//! providing a non-stochastic harness that validates implementations against
//! specs using runtime traces.
//!
//! ## What `SpecGate` can do
//!
//! - Assert on the runtime traces operations emit, with a rich set of matcher
//!   operators (`$gt`, `$contains`, `$match`, `$type`, `$not`, ...) and position
//!   directives (`$unordered`, `$anywhere`).
//! - Model state via setups, mock dependencies, and capture structured values —
//!   structs, enums, lists/maps/sets, and the built-in `value` type.
//! - Optional inputs with defaults, property-based cases, and async operations
//!   (driven on a `smol` or `tokio` runtime).
//! - Group a crate's public API into components, and run in reverse with
//!   `specgate extract` to derive a spec from annotated code.
//!
//! ## Usage
//!
//! Add to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! specgate = "0.1"
//!
//! [dev-dependencies]
//! specgate = { version = "0.1", features = ["harness"] }
//! ```
//!
//! Annotate your code:
//!
//! ```rust,ignore
//! use specgate::*;
//!
//! // Declare the component this crate implements (once, at the crate root).
//! spec_component!("counter.service");
//!
//! // Capture selected fields of a struct into the trace.
//! #[derive(SpecEvent)]
//! struct Count {
//!     #[spec_event]
//!     value: i32,
//! }
//!
//! struct Counter {
//!     value: i32,
//! }
//!
//! // A setup builds the receiver for stateful (method) operations;
//! // `#[spec_input]` gives a parameter a language-neutral spec name.
//! #[spec_setup("counter")]
//! fn new_counter(#[spec_input("start")] initial: i32) -> Counter {
//!     Counter { value: initial }
//! }
//!
//! impl Counter {
//!     #[spec_operation("increment")]
//!     fn increment(&mut self, #[spec_input("by")] delta: i32) -> Count {
//!         self.value += delta;
//!         spec_trace!("after_add", &self.value); // inline trace checkpoint
//!         Count { value: self.value }
//!     }
//! }
//!
//! // A free-function operation whose dependency call is mocked: the spec
//! // supplies the response, so the real `Directory` is never hit under test.
//! #[spec_operation("lookup")]
//! fn lookup(dir: &Directory, id: &str) -> String {
//!     #[spec_mock("dir")]
//!     let name = dir.find(id);
//!     name
//! }
//!
//! struct Directory;
//! impl Directory {
//!     fn find(&self, _id: &str) -> String {
//!         unreachable!("mocked under test")
//!     }
//! }
//! ```
//!
//! Run your spec:
//!
//! ```rust,ignore
//! #[test]
//! fn spec_passes() {
//!     let result = specgate::run_spec("specs/my-component.spec.yaml");
//!     assert!(matches!(result, specgate::RunOutcome::Complete { .. }));
//! }
//! ```
//!
//! ## Annotation surface
//!
//! - `#[spec_operation("name")]` — mark a function as a spec operation.
//! - `#[spec_setup("name")]` — build the receiver for stateful operations.
//! - `#[spec_mock(...)]` — inject a table-driven mock dependency.
//! - `#[derive(SpecEvent)]` + `#[spec_event]` — capture struct/enum fields.
//! - `#[spec_input("name")]` — give a parameter a language-neutral spec name.
//! - `spec_component!("name")` — declare the crate's component.
//! - `spec_trace!(...)` — emit an inline trace checkpoint.
//!
//! ## Property Tests
//!
//! Specs can declare property-based test cases that generate random inputs
//! and verify invariants across many iterations:
//!
//! ```yaml
//! cases:
//!   - name: add_commutative
//!     kind: property
//!     runs: 100
//!     generators:
//!       a: i32[-1000, 1000]
//!       b: i32[-1000, 1000]
//!     calls:
//!       forward: { operation: add, inputs: { a: "{a}", b: "{b}" } }
//!       reversed: { operation: add, inputs: { a: "{b}", b: "{a}" } }
//!     expected:
//!       - $assert: "forward.$result == reversed.$result"
//! ```
//!
//! Generator types: `i32[min, max]`, `f64[min, max]`, `bool`,
//! `string[min_len, max_len]`, `string[min, max, pattern: "regex"]`,
//! `oneof["a", "b"]`, `list[type, len: min..max]`,
//! `set[type, size: min..max]`, `map[key, value, size: min..max]`, `optional[type]`.
//!
//! On failure, the `CaseResult` includes a `counterexample` with the shrunk
//! generator values that triggered the assertion failure, plus traces from
//! the failing run.
//!
//! ## CLI
//!
//! Install the companion CLI for command-line validation and execution:
//!
//! ```bash
//! cargo install specgate-cli
//! specgate validate specs/
//! specgate run specs/my-component.spec.yaml
//! specgate extract path/to/crate -o specs/derived.spec.yaml
//! ```
//!
//! ## Features
//!
//! - **`harness`** — enables `run_spec()` and the test harness (add to `[dev-dependencies]`)
//! - **`trace`** — enables runtime trace collection (required for harness, zero-cost when off)
//!
//! ## Learn more
//!
//! - [Knowledge base](https://github.com/schgoo/specgate/tree/main/docs/knowledge)
//!   — spec format, annotations, bindings, extraction, and more.
//! - [Fixture Catalog](https://github.com/schgoo/specgate/blob/main/docs/knowledge/fixtures.md)
//!   — every feature demonstrated by a runnable fixture (the source-of-truth examples).

// Public API — annotations
pub use specgate_annotations::{SpecEvent, emit_event, spec_component, spec_mock, spec_operation, spec_setup, spec_trace};

// Internal — needed by macro expansions but not user-facing
#[doc(hidden)]
pub use specgate_annotations::{ToSpecValue, TraceEvent, Value, emit_event_v, emit_run, mock_lookup, reset, set_mock, take_traces};

// The proc macros expand to `::specgate::__rt::...` so this module must exist.
#[doc(hidden)]
pub mod __rt {
    pub use specgate_annotations::__rt::*;
}

// Public API — harness (behind "harness" feature)
#[cfg(feature = "harness")]
pub use specgate_harness::{CaseStatus, RunOutcome, run_spec};
