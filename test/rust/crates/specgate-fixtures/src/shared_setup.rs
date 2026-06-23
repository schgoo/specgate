// Shared-setup scenarios: one setup function filling several same-typed
// parameters via stacked `#[spec_setup(..., fills = ...)]` attributes.
use specgate::*;

#[derive(SpecEvent)]
pub struct BoxVal {
    #[spec_event]
    pub value: i32,
}

// Scenario 1: one reusable setup with a construction input fills two params;
// each fill takes a distinct flat input named `<param>_<fills>`.
#[spec_setup("combine", fills = "left")]
#[spec_setup("combine", fills = "right")]
pub fn make_box(start: i32) -> BoxVal {
    BoxVal { value: start }
}

#[spec_operation("combine")]
pub fn combine(left: &BoxVal, right: &BoxVal) -> i32 {
    left.value + right.value
}

// Scenario 2: a parameterless setup fills three params (same value each).
#[spec_setup("combine_three", fills = "a")]
#[spec_setup("combine_three", fills = "b")]
#[spec_setup("combine_three", fills = "c")]
pub fn make_unit() -> BoxVal {
    BoxVal { value: 1 }
}

#[spec_operation("combine_three")]
pub fn combine_three(a: &BoxVal, b: &BoxVal, c: &BoxVal) -> i32 {
    a.value + b.value + c.value
}
