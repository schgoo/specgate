// Language-neutral input names via `#[spec_input(...)]` on parameters.
// The spec uses `numerator`/`denominator`/`factor`/`value`; the Rust code uses
// its own parameter names.
use specgate::*;

#[spec_operation("divide")]
pub fn divide(#[spec_input("numerator")] a: i32, #[spec_input("denominator")] b: i32) -> i32 {
    a / b
}

#[spec_setup("scale")]
pub fn make_scaler(#[spec_input("factor")] f: i32) -> Scaler {
    Scaler { factor: f }
}

#[derive(SpecEvent)]
pub struct Scaler {
    #[spec_event]
    pub factor: i32,
}

impl Scaler {
    #[spec_operation("scale")]
    pub fn scale(&self, #[spec_input("value")] v: i32) -> i32 {
        self.factor * v
    }
}
