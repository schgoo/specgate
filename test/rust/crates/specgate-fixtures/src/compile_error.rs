// Source that doesn't compile — syntax error.
use specgate::*;

#[spec_operation("broken")]
fn broken() -> i32 {
    let x = ;  // syntax error
    x
}
