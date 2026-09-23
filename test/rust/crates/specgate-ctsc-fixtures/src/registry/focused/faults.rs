use specgate::spec_operation;

#[spec_operation("explode", spec = "fixture.faults")]
pub fn explode() -> i32 {
    panic!("fixture fault")
}

#[test]
fn explode_unwinds_with_its_fixture_fault_message() {
    let unwound = std::panic::catch_unwind(explode).expect_err("explode must unwind");
    let message = unwound
        .downcast_ref::<&str>()
        .copied()
        .expect("fixture fault payload");
    assert_eq!(message, "fixture fault");
}
