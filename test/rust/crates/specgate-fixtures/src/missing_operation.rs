// Missing operation annotation — no spec_operation on any function.
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
    // Note: no #[spec_operation] here
    pub fn increment(&mut self) {
        self.count += 1;
    }
}
