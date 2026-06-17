// Multi-step state machine — two operations in sequence.
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
    #[spec_operation("increment")]
    fn increment(&mut self) {
        self.count += 1;
    }

    #[spec_operation("decrement")]
    fn decrement(&mut self) {
        self.count -= 1;
    }
}
