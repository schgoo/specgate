// Scalar built-in types: i64, bool, and the universal `value`. Surfaces the
// scalar corners of the type system the other fixtures don't reach — 64-bit
// integers, booleans, and the runtime `Value` (spec `value`) as an operation
// result — so discovery can self-describe every scalar built-in.
use specgate::*;

#[spec_operation("classify", spec = "fixture.scalar_types")]
pub fn classify(id: i64, active: bool) -> Value {
    if active {
        Value::Integer(id)
    } else {
        Value::Bool(false)
    }
}
