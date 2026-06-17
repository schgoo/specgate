// Multiple mutations within a single operation.
// The system must capture every mutation of count, not just boundaries.
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
    #[spec_operation("increment_twice")]
    fn increment_twice(&mut self) {
        self.count += 1;
        self.count += 1;
    }
}
