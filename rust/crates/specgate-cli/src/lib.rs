//! Command-line interface for `SpecGate`'s CTSC-native workflow.
//!
//! ```text
//! specgate discover <binding.yaml> --component <id> --registry-id <id> --registry-version <version> --out <registry.ctsc.json> [--target <name>]
//! specgate capture <binding.yaml> --out <dir> [--target <name>] [--component <id>]
//! specgate replay <capture-dir> <candidate-binding.yaml> --out <candidate.otlp.json> [--target <name>]
//! specgate validate registry <registry.json> [--import <registry.json>]...
//! specgate validate trace <trace.otlp.json|trace.otlp.jsonl>
//! specgate validate linked <trace> <root-registry> [--import <registry.json>]...
//! specgate validate bundle <capture-dir>
//! specgate compare <reference-trace> <candidate-trace> [--registry <root-registry>] [--import <registry.json>]...
//! ```
//!
//! `discover` exports a deterministic CTSC registry from Rust link-time or C#
//! compiled-assembly metadata. `capture` runs ordinary Rust tests in isolation
//! and records the selected component's top-level operation subtrees from
//! passing native scenarios, keeping nested calls into other annotated
//! components verbatim. `replay` verifies a capture bundle,
//! statically links its top-level semantic inputs to a Rust candidate, and
//! emits an independent deterministic CTSC trace. `validate` provides native
//! CTSC 0.2 Registry, Trace Core, Linked, and capture-bundle validation.
//! `compare` applies deterministic `ctsc.strict/0.1.0` semantics. No command
//! reads `.spec.yaml` or invokes Python. The library capture entry point accepts
//! one owned [`CaptureRequest`], constructed with a validated builder, and
//! returns `Result<CaptureReport, CaptureError>`.
//!
//! # Examples
//!
//! ```no_run
//! use specgate_cli::{CapturePaths, CaptureRequest, capture};
//!
//! let request = CaptureRequest::builder(CapturePaths {
//!     binding: "binding.yaml".into(),
//!     out: "capture".into(),
//! })
//!     .component("example.math")
//!     .build()?;
//! let report = capture(request)?;
//! println!("registry: {}", report.registry_path.display());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

specgate::spec_component!("specgate.cli");

mod capture_failure;
#[path = "capture/mod.rs"]
mod capture_impl;
pub mod discover;
pub mod replay;
mod system;

pub use capture_failure::CaptureError;
pub use capture_impl::{CapturePaths, CaptureReport, CaptureRequest, CaptureRequestBuilder, capture, capture_with};

pub use system::{CommandEnvironment, Discovery, Execution};

/// Render a capture result using the stable CLI line protocol.
#[doc(hidden)]
#[must_use]
pub fn format_capture(outcome: &Result<CaptureReport, CaptureError>) -> String {
    match outcome {
        Ok(report) => format!(
            "Complete(component={}, scenarios={}, operations={}, registry={}, trace={}, manifest={})\n",
            report.component_id(),
            report.scenarios(),
            report.operations(),
            report.registry_path.display(),
            report.trace_path.display(),
            report.manifest_path.display()
        ),
        Err(error) => format!("Error({})\n", error.diagnostic()),
    }
}

#[cfg(test)]
mod goldens;
