// Setup with input parameter — initial count is configurable.
use specgate_annotations::*;

#[spec_setup("make_counter")]
pub fn make_counter(initial: i32) -> Counter {
    Counter { count: initial }
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
