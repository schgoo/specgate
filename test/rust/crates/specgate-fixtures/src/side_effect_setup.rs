// Side-effect setup: the setup has an effect the harness just needs to run;
// its return value is not consumed by the operation.
use specgate::*;
use std::sync::atomic::{AtomicI32, Ordering};

static FLAG: AtomicI32 = AtomicI32::new(0);

#[spec_setup("read_flag")]
pub fn enable_flag() {
    FLAG.store(1, Ordering::SeqCst);
}

#[spec_operation("read_flag")]
pub fn read_flag() -> i32 {
    FLAG.load(Ordering::SeqCst)
}
