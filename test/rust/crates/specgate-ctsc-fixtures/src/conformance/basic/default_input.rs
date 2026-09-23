use specgate::{SpecEvent, spec_operation};

#[spec_operation("scale", spec = "fixture.default_input")]
pub fn scale(value: i32, factor: i32) -> i32 {
    value * factor
}

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.default_input")]
pub struct Offset {
    #[spec_event]
    pub dx: i32,
    #[spec_event]
    pub dy: i32,
}

#[spec_operation("shift", spec = "fixture.default_input")]
pub fn shift(base: i32, by: Offset) -> i32 {
    base + by.dx + by.dy
}

#[test]
fn historical_default_values_are_explicit_capture_inputs() {
    assert_eq!(scale(6, 2), 12);
    assert_eq!(shift(10, Offset { dx: 1, dy: 1 }), 12);
}
