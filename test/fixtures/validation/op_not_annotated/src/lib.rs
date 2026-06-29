use specgate_annotations::spec_operation;
specgate_annotations::spec_component!("fixture.validation.op_not_annotated");

#[spec_operation("other")]
pub fn other() -> i32 {
    0
}
