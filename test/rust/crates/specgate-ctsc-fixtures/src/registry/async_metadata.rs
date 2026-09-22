use specgate::spec_operation;
use std::time::Duration;

#[spec_operation("smol_delay", spec = "fixture.async_smol_timer")]
pub async fn smol_delay() -> String {
    smol::Timer::after(Duration::from_millis(1)).await;
    "smol done".to_string()
}

#[spec_operation("tokio_delay", spec = "fixture.async_tokio_timer")]
pub async fn tokio_delay() -> String {
    tokio::time::sleep(Duration::from_millis(1)).await;
    "tokio done".to_string()
}

#[test]
fn runtime_specific_async_operations_are_metadata_only_for_capture() {
    for (component, name) in [
        ("fixture.async_smol_timer", "smol_delay"),
        ("fixture.async_tokio_timer", "tokio_delay"),
    ] {
        let operation = specgate::__rt::SPECGATE_OPS
            .iter()
            .find(|operation| operation.component == component && operation.name == name)
            .unwrap_or_else(|| panic!("missing async metadata for {component}::{name}"));
        assert!(operation.is_async);
        assert!(!operation.is_setup);
    }
}
