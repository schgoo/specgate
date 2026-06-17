// StateMachine operation with before/after field capture.
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
}
