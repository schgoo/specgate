use specgate::*;
specgate::spec_component!("test.optional_input");

#[spec_operation("scale")]
pub fn scale(value: i32, factor: i32) -> i32 {
    value * factor
}
