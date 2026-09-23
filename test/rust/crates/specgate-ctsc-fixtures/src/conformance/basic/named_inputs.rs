use specgate::{SpecEvent, spec_operation, spec_setup};

#[spec_operation("divide", spec = "fixture.named_inputs")]
pub fn divide(
    #[spec_input("numerator")] dividend: i32,
    #[spec_input("denominator")] divisor: i32,
) -> i32 {
    dividend / divisor
}

#[derive(Debug, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.named_inputs")]
pub struct Scaler {
    #[spec_event]
    pub factor: i32,
}

#[spec_setup("scale", spec = "fixture.named_inputs")]
pub fn make_scaler(#[spec_input("factor")] multiplier: i32) -> Scaler {
    Scaler { factor: multiplier }
}

impl Scaler {
    #[spec_operation("scale", spec = "fixture.named_inputs")]
    pub fn scale(&self, #[spec_input("value")] operand: i32) -> i32 {
        self.factor * operand
    }
}

#[test]
fn language_neutral_input_names_preserve_behavior() {
    assert_eq!(divide(20, 4), 5);
    let scaler = make_scaler(3);
    assert_eq!(scaler.scale(7), 21);
}
