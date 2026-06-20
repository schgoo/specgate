// Source that doesn't compile — syntax error.
use specgate_annotations::*;

#[spec_operation("broken")]
fn broken() -> i32 {
    let x = ;  // syntax error
    x
}
