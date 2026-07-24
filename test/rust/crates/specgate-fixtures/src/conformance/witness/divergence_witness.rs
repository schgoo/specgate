// Multi-target divergence witness. Both bound targets agree on the asserted
// `value` but deliberately differ on the non-asserted `engine` field, so the
// case satisfies `expected:` under every target yet the targets' full traces
// differ. This distinguishes trace-agreement divergence (a csharp
// TargetFailure is reported) from status-agreement (which would wrongly pass).
use specgate::*;

#[derive(SpecEvent)]
pub struct EngineInfo {
    #[spec_event(name = "value")]
    pub value: i32,
    #[spec_event(name = "engine")]
    pub engine: String,
}

#[spec_operation("engine_info", spec = "fixture.divergence_witness")]
pub fn engine_info() -> EngineInfo {
    EngineInfo {
        value: 10,
        engine: "rust".to_string(),
    }
}
