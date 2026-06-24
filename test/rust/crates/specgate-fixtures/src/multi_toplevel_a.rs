// Operation `alpha` — lives in its own top-level file. Paired with
// `multi_toplevel_b.rs` (operation `beta`); a single spec references both,
// exercising the harness's ability to merge operations split across separate
// top-level source files.
use specgate::*;

#[spec_operation("alpha")]
pub fn alpha(x: i32) -> i32 {
    x + 1
}
