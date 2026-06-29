use specgate_annotations::spec_operation;
specgate_annotations::spec_component!("fixture.validation.no_cases");

#[spec_operation("noop")]
pub fn noop() -> i32 {
    0
}
