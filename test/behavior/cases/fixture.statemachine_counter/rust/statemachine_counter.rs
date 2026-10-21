use specgate::{SpecEvent, spec_operation, spec_setup};

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.statemachine_counter")]
pub struct Counter {
    #[spec_event]
    pub count: i32,
}

#[spec_setup("increment", spec = "fixture.statemachine_counter")]
pub fn make_counter() -> Counter {
    Counter { count: 0 }
}

impl Counter {
    #[spec_operation("increment", spec = "fixture.statemachine_counter")]
    pub fn increment(&mut self) {
        self.count += 1;
        observe_state(self);
    }
}

#[spec_operation("observe_state", spec = "fixture.statemachine_counter")]
pub fn observe_state(counter: &Counter) -> Counter {
    counter.clone()
}

#[test]
fn state_machine_transitions_from_zero_to_one() {
    let mut counter = make_counter();
    counter.increment();
    assert_eq!(counter.count, 1);
}
