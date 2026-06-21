// StateMachine operation with before/after field capture.
use specgate::*;

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
}
