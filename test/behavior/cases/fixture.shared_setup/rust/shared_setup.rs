use specgate::{SpecEvent, spec_operation, spec_setup};

#[derive(Debug, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.shared_setup")]
pub struct BoxVal {
    #[spec_event]
    pub value: i32,
}

#[spec_setup("combine", fills = "left", component = "fixture.shared_setup")]
#[spec_setup("combine", fills = "right", component = "fixture.shared_setup")]
pub fn make_box() -> BoxVal {
    BoxVal { value: 2 }
}

#[spec_operation("combine", component = "fixture.shared_setup")]
pub fn combine(left: &BoxVal, right: &BoxVal) -> i32 {
    left.value + right.value
}

#[spec_setup("combine_three", fills = "a", component = "fixture.shared_setup")]
#[spec_setup("combine_three", fills = "b", component = "fixture.shared_setup")]
#[spec_setup("combine_three", fills = "c", component = "fixture.shared_setup")]
pub fn make_unit() -> BoxVal {
    BoxVal { value: 1 }
}

#[spec_operation("combine_three", component = "fixture.shared_setup")]
pub fn combine_three(a: &BoxVal, b: &BoxVal, c: &BoxVal) -> i32 {
    a.value + b.value + c.value
}

#[test]
fn one_setup_function_can_fill_multiple_parameters() {
    let left = make_box();
    let right = make_box();
    assert_eq!(combine(&left, &right), 4);
    let a = make_unit();
    let b = make_unit();
    let c = make_unit();
    assert_eq!(combine_three(&a, &b, &c), 3);
}
