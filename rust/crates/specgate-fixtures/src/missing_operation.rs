// Missing operation annotation — no spec_operation on any function.
use specgate_annotations::*;

#[spec_setup("make_counter")]
fn make_counter() -> Counter {
    Counter { count: 0 }
}

struct Counter {
    #[spec_event]
    count: i32,
}

impl Counter {
    // Note: no #[spec_operation] here
    fn increment(&mut self) {
        self.count += 1;
    }
}
