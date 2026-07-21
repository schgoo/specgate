// Simple-output setup: the setup returns a primitive that fills an operation
// parameter by type. Allowed but spurious — a literal input would usually do.
use specgate::*;

#[spec_setup("double")]
pub fn make_n() -> i32 {
    21
}

#[spec_operation("double")]
pub fn double(n: i32) -> i32 {
    n * 2
}
