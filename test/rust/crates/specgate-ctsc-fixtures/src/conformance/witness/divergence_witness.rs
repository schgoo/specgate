use specgate::{SpecEvent, spec_operation};

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.divergence_witness")]
pub struct EngineInfo {
    #[spec_event]
    pub value: i32,
    #[spec_event]
    pub engine: String,
}

#[spec_operation("engine_info", spec = "fixture.divergence_witness")]
pub fn engine_info() -> EngineInfo {
    EngineInfo {
        value: 10,
        engine: "rust".to_string(),
    }
}

#[test]
fn rust_target_keeps_its_deliberate_engine_witness() {
    assert_eq!(
        engine_info(),
        EngineInfo {
            value: 10,
            engine: "rust".to_string()
        }
    );
}
