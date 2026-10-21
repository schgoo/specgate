use specgate::{SpecEvent, spec_operation, spec_setup};

#[derive(Debug, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.setup_with_params")]
pub struct Counter {
    #[spec_event]
    pub count: i32,
}

#[spec_setup("increment", spec = "fixture.setup_with_params")]
pub fn make_counter(initial: i32) -> Counter {
    Counter { count: initial }
}

impl Counter {
    #[spec_operation("increment", spec = "fixture.setup_with_params")]
    pub fn increment(&mut self) {
        self.count += 1;
        observe_count(self.count);
    }
}

#[spec_operation("observe_count", spec = "fixture.setup_with_params")]
pub fn observe_count(count: i32) -> i32 {
    count
}

#[test]
fn parameterized_setup_controls_initial_state() {
    let mut counter = make_counter(9);
    counter.increment();
    assert_eq!(counter.count, 10);
}
