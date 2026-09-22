//! An operation whose return type projects native values but is never
//! registered as a semantic `SpecEvent`, so no CTSC registry type resolves it.

use specgate::{ToNativeValue, Value, spec_operation};

pub struct Gadget {
    pub value: i32,
}

impl ToNativeValue for Gadget {
    fn to_native_value(&self) -> Value {
        Value::Integer(i64::from(self.value))
    }
}

#[spec_operation("build", spec = "fixture.unresolved_type")]
pub fn build() -> Gadget {
    Gadget { value: 7 }
}
