//! Self-hosting entry point: exposes the harness's own `run_spec` as a spec
//! operation named `"run_spec"`, returning a `SpecEvent`-deriving outcome so the
//! harness can validate ITS OWN spec (`specs/specgate.harness.spec.yaml`)
//! through its own pipeline.
//!
//! The harness's real `RunOutcome` / `CaseResult` / `TraceEvent` types do not
//! derive `SpecEvent`, so they are mirrored here with local `SpecEvent` types
//! whose `to_spec_value` yields the structured `$result` the harness spec
//! asserts: `{ Complete: { results: [ { name, status, traces } ] } }` /
//! `{ Error: { reason } }`.

use specgate::{SpecEvent, ToSpecValue, Value, spec_operation};

/// Identity adapter: the runtime `Value` is already a spec value but does not
/// impl `ToSpecValue` (which every `#[derive(SpecEvent)]` field needs).
pub struct SpecVal(pub Value);

impl std::fmt::Debug for SpecVal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl ToSpecValue for SpecVal {
    fn to_spec_value(&self) -> Value {
        self.0.clone()
    }
}

#[derive(Debug, SpecEvent)]
pub enum SelfHostTrace {
    Run { operation: String },
    Event { name: String, value: SpecVal },
}

#[derive(Debug, SpecEvent)]
pub struct SelfHostCaseResult {
    #[spec_event]
    pub name: String,
    #[spec_event]
    pub status: String,
    #[spec_event]
    pub traces: Vec<SelfHostTrace>,
}

#[derive(Debug, SpecEvent)]
pub enum SelfHostOutcome {
    Complete { results: Vec<SelfHostCaseResult> },
    Error { reason: String },
}

fn convert_trace(t: specgate_harness::TraceEvent) -> SelfHostTrace {
    match t {
        specgate_harness::TraceEvent::Run { operation } => SelfHostTrace::Run { operation },
        specgate_harness::TraceEvent::Event { name, value } => SelfHostTrace::Event {
            name,
            value: SpecVal(value),
        },
    }
}

#[spec_operation("run_spec")]
pub fn run_spec(#[spec_input("spec")] spec_path: &str) -> SelfHostOutcome {
    // The harness spec uses repo-root-relative paths, but the generated runner
    // executes with the harness scratch dir
    // (`<repo>/rust/target/specgate-harness/<stem>`) as its `CARGO_MANIFEST_DIR`.
    // Resolve relative paths against the repo root, four levels up.
    let resolved = {
        let p = std::path::Path::new(spec_path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            let mut root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            root.pop();
            root.pop();
            root.pop();
            root.pop();
            root.join(p)
        }
    };
    let resolved = resolved.to_string_lossy().into_owned();
    match specgate_harness::run_spec(&resolved) {
        specgate_harness::RunOutcome::Complete { results } => SelfHostOutcome::Complete {
            results: results
                .into_iter()
                .map(|r| SelfHostCaseResult {
                    name: r.name,
                    status: r.status.as_str().to_string(),
                    traces: r.traces.into_iter().map(convert_trace).collect(),
                })
                .collect(),
        },
        specgate_harness::RunOutcome::Error { reason } => SelfHostOutcome::Error { reason },
    }
}
