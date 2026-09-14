// Setup with input parameter — initial count is configurable.
use specgate::*;

#[spec_setup("increment", spec = "fixture.setup_with_params")]
pub fn make_counter(initial: i32) -> Counter {
    Counter { count: initial }
}

#[derive(SpecEvent)]
#[spec_component("fixture.setup_with_params")]
pub struct Counter {
    #[spec_event]
    pub count: i32,
}

impl Counter {
    #[spec_operation("increment", spec = "fixture.setup_with_params")]
    pub fn increment(&mut self) {
        self.count += 1;
    }
}
