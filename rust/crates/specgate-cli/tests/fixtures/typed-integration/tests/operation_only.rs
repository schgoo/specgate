#![deny(warnings)]
#![doc = include_str!("operation_only_support/value.txt")]

specgate::spec_component!("fixture.operation_only");

#[path = "operation_only_support/mod.rs"]
mod support;

pub struct Associated;

impl Associated {
    #[specgate::spec_operation("identity", spec = "fixture.operation_only")]
    pub fn identity(value: i32) -> i32 {
        value
    }
}

#[specgate::spec_operation("increment", spec = "fixture.operation_only")]
pub fn increment(value: i32) -> i32 {
    assert!(file!().ends_with("operation_only.rs"));
    value + support::delta()
}

#[test]
fn operation_without_setup() {
    assert_eq!(increment(8), 9);
    assert_eq!(include_str!("operation_only_support/value.txt").trim(), "1");
    let _ = Associated::identity as fn(i32) -> i32;
}
