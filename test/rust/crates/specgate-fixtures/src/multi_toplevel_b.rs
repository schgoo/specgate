// Operation `beta` — lives in its own top-level file (see `multi_toplevel_a.rs`).
use specgate::*;

#[spec_operation("beta")]
pub fn beta(x: i32) -> i32 {
    x * 2
}
