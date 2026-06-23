// Self-host wrapper: exposes `specgate_harness::run_spec` as a spec operation
// named "run_spec", returning a SpecEvent-deriving outcome so the harness can
// validate ITS OWN spec through its own pipeline.
//
// The harness's real `RunOutcome` / `CaseResult` types live in specgate-harness
// and don't derive SpecEvent. Here we mirror them with local SpecEvent types so
// codegen emits a structured `$result` matching the harness spec's documented
// `outcome:` shape: `{ Complete: { results: [ { name, status } ] } }` /
// `{ Error: { reason } }`.
use specgate::*;

/// Identity adapter: the runtime `Value` is already a spec value, but it does
/// not impl `ToSpecValue` (which every `#[derive(SpecEvent)]` field needs).
/// This local newtype bridges that gap inside test code without touching the
/// runtime crate.
pub struct SpecVal(pub Value);

impl ToSpecValue for SpecVal {
    fn to_spec_value(&self) -> Value {
        self.0.clone()
    }
}

#[derive(SpecEvent)]
pub enum SelfHostTrace {
    Run { operation: String },
    Event { name: String, value: SpecVal },
}

#[derive(SpecEvent)]
pub struct SelfHostCaseResult {
    #[spec_event]
    pub name: String,
    #[spec_event]
    pub status: String,
    #[spec_event]
    pub traces: Vec<SelfHostTrace>,
}

#[derive(SpecEvent)]
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
    // executes from an arbitrary working directory. Resolve relative paths
    // against the repo root, derived from this crate's compile-time location
    // (test/rust/crates/specgate-fixtures → up four levels).
    let resolved = {
        let p = std::path::Path::new(spec_path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            let mut root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            root.pop(); // specgate-fixtures
            root.pop(); // crates
            root.pop(); // rust
            root.pop(); // test
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
