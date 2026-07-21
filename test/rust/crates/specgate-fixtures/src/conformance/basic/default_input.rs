// Optional operation inputs via spec-level `default:` values. When a case omits
// a defaulted input, the harness materializes the declared default (scalar or
// complex) exactly as if the case had provided it.
use serde::{Deserialize, Serialize};
use specgate::*;

/// Scalar-default demo: `factor` defaults to 2 in the spec.
#[spec_operation("scale")]
pub fn scale(value: i32, factor: i32) -> i32 {
    value * factor
}

#[derive(Serialize, Deserialize, SpecEvent)]
pub struct Offset {
    #[spec_event]
    pub dx: i32,
    #[spec_event]
    pub dy: i32,
}

/// Complex-default demo: `by` defaults to `{dx: 1, dy: 1}` in the spec.
#[spec_operation("shift")]
pub fn shift(base: i32, by: Offset) -> i32 {
    base + by.dx + by.dy
}
