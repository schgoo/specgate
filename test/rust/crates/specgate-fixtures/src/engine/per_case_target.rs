//! Own `add` vehicle for the `fixture.per_case_target` component. The
//! per_case_target spec exercises per-case target dispatch (default target for
//! this `add`, alt target for `greet` which lives in the alt crate). `add` is
//! ambiguous across components in the default crate, so this component owns its
//! own copy for exact (component, op) resolution.
use specgate::*;

/// Adds two integers; resolved on the default binding target.
#[spec_operation("add", spec = "fixture.per_case_target")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
