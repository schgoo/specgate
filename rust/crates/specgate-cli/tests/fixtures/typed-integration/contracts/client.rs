use typed_client::*;

specgate::spec_component!("fixture.integration_local");

#[path = "../../typed-async/src/setup.rs"]
mod annotated_environment;

#[specgate::spec_operation("increment", spec = "fixture.integration_local")]
pub fn increment(value: i32) -> i32 {
    value + 1
}

#[test]
fn integration_local_operation() {
    assert_eq!(increment(4), 5);
}
