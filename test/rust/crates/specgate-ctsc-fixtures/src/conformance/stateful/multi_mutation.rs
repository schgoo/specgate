use specgate::{SpecEvent, spec_operation, spec_setup};

#[derive(Debug, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.multi_mutation")]
pub struct Counter {
    #[spec_event]
    pub count: i32,
}

#[spec_setup("increment_twice", spec = "fixture.multi_mutation")]
pub fn make_counter() -> Counter {
    Counter { count: 0 }
}

impl Counter {
    #[spec_operation("increment_twice", spec = "fixture.multi_mutation")]
    pub fn increment_twice(&mut self) {
        self.count += 1;
        observe_count("after_first", self.count);
        self.count += 1;
        observe_count("after_second", self.count);
    }
}

#[spec_operation("observe_count", spec = "fixture.multi_mutation")]
pub fn observe_count(stage: &str, count: i32) -> i32 {
    assert!(matches!(stage, "after_first" | "after_second"));
    count
}

#[test]
fn both_mutations_are_observable() {
    let mut counter = make_counter();
    counter.increment_twice();
    assert_eq!(counter.count, 2);
}
