use specgate_annotations::spec_operation;

#[spec_operation("noop")]
pub fn noop() -> i32 {
    0
}
