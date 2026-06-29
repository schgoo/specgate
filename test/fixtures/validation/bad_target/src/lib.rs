use specgate_annotations::spec_operation;
specgate_annotations::spec_component!("fixture.validation.bad_target");

#[spec_operation("noop")]
pub fn noop() -> i32 {
    0
}
