//! Own `add` vehicle for the `fixture.provenance_example` component, so its
//! provenance case resolves via (component, op) metadata instead of borrowing
//! another component's `add` (which is now ambiguous across components).
use specgate::*;

/// Adds two integers; the spec attaches source-provenance metadata to the case.
#[spec_operation("add", spec = "fixture.provenance_example")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
