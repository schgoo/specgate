//! Command-line interface for `SpecGate`'s CTSC-native workflow.
//!
//! ```text
//! specgate discover <binding.yaml> --component <id> --registry-id <id> --registry-version <version> --out <registry.ctsc.json> [--target <name>]
//! specgate capture <binding.yaml> --out <dir> [--target <name>] [--component <id>]
//! specgate replay <capture-dir> <candidate-binding.yaml> --out <candidate.otlp.json> [--target <name>]
//! ```
//!
//! `discover` exports a deterministic CTSC registry from Rust link-time or C#
//! compiled-assembly metadata. `capture` runs ordinary Rust tests in isolation
//! and records passing native scenarios. `replay` verifies a capture bundle,
//! statically links its top-level semantic inputs to a Rust candidate, and
//! emits an independent deterministic CTSC trace. No command reads
//! `.spec.yaml`.

specgate::spec_component!("specgate.cli");

pub mod capture;
pub mod discover;
pub mod replay;

pub use capture::{CaptureOutcome, CaptureReport, capture};
pub use discover::{DiscoverOutcome, DiscoverReport, discover};
pub use replay::{ReplayInvocationPlan, ReplayOutcome, ReplayReport, replay};
