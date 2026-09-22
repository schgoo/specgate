use specgate::{SpecEvent, spec_operation, spec_setup};

#[derive(Debug, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.multi_step")]
pub struct Counter {
    #[spec_event]
    pub count: i32,
}

#[spec_setup("increment", spec = "fixture.multi_step")]
#[spec_setup("decrement", spec = "fixture.multi_step")]
pub fn make_counter() -> Counter {
    Counter { count: 0 }
}

impl Counter {
    #[spec_operation("increment", spec = "fixture.multi_step")]
    pub fn increment(&mut self) {
        self.count += 1;
        observe_count("increment", self.count);
    }

    #[spec_operation("decrement", spec = "fixture.multi_step")]
    pub fn decrement(&mut self) {
        self.count -= 1;
        observe_count("decrement", self.count);
    }
}

#[spec_operation("observe_count", spec = "fixture.multi_step")]
pub fn observe_count(operation: &str, count: i32) -> i32 {
    assert!(matches!(operation, "increment" | "decrement"));
    count
}

#[test]
fn sequential_operations_share_one_setup_instance() {
    let mut counter = make_counter();
    counter.increment();
    counter.increment();
    counter.decrement();
    assert_eq!(counter.count, 1);
}
