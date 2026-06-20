// Multi-step state machine — two operations in sequence.
use specgate_annotations::*;

#[spec_setup("make_counter")]
pub fn make_counter() -> Counter {
    Counter { count: 0 }
}

#[derive(SpecEvent)]
pub struct Counter {
    #[spec_event]
    pub count: i32,
}

impl Counter {
    #[spec_operation("increment")]
    pub fn increment(&mut self) {
        self.count += 1;
    }

    #[spec_operation("decrement")]
    pub fn decrement(&mut self) {
        self.count -= 1;
    }
}
