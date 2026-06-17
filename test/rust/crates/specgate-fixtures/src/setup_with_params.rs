// Setup with input parameter — initial count is configurable.
use specgate_annotations::*;

#[spec_setup("make_counter")]
fn make_counter(initial: i32) -> Counter {
    Counter { count: initial }
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
