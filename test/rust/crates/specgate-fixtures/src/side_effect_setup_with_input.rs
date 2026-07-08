// Parameterized side-effect setup: the setup writes its construction input into
// global state, which the operation then reads back. The value comes from the
// case's `setup:` map, proving global-state-with-a-value is established
// deterministically by the harness (not ambient/external state).
use specgate::*;
use std::sync::atomic::{AtomicI32, Ordering};

static LIMIT: AtomicI32 = AtomicI32::new(0);

#[spec_setup("check_limit")]
pub fn set_limit(limit: i32) {
    LIMIT.store(limit, Ordering::SeqCst);
}

#[spec_operation("check_limit")]
pub fn check_limit() -> i32 {
    LIMIT.load(Ordering::SeqCst)
}
