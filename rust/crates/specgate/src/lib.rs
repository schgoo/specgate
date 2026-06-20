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

// Re-export annotations (always available)
pub use specgate_annotations::{
    SpecEvent, ToSpecValue, TraceEvent, Value, emit_event, emit_event_v, emit_run, mock_lookup, reset, set_mock, spec_mock, spec_operation,
    spec_setup, spec_trace, take_traces,
};

// Re-export harness (behind "harness" feature)
#[cfg(feature = "harness")]
pub use specgate_harness::{CaseLevel, CaseResult, CaseStatus, RunOutcome, Source, run_spec};
