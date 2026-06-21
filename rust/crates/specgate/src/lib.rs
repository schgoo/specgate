//! # `SpecGate`
//!
//! Deterministic spec-based verification for LLM-implemented code.
//!
//! Engineers write specs. LLMs implement them. `SpecGate` closes the gap by
//! providing a non-stochastic harness that validates implementations against
//! specs using runtime traces.
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
//! use specgate::{spec_operation, SpecEvent};
//!
//! #[derive(SpecEvent)]
//! struct Point { x: i32, y: i32 }
//!
//! #[spec_operation("add_points")]
//! fn add_points(a: Point, b: Point) -> Point {
//!     Point { x: a.x + b.x, y: a.y + b.y }
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
//! ```
//!
//! ## Features
//!
//! - **`harness`** — enables `run_spec()` and the test harness (add to `[dev-dependencies]`)
//! - **`trace`** — enables runtime trace collection (required for harness, zero-cost when off)

// Public API — annotations
pub use specgate_annotations::{SpecEvent, emit_event, spec_mock, spec_operation, spec_setup, spec_trace};

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
