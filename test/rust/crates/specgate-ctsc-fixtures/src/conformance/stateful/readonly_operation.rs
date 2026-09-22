use specgate::{SpecEvent, spec_operation, spec_setup};

#[derive(Debug, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.readonly_operation")]
pub struct Counter {
    #[spec_event]
    pub count: i32,
}

#[spec_setup("get_count", spec = "fixture.readonly_operation")]
pub fn make_counter() -> Counter {
    Counter { count: 42 }
}

impl Counter {
    #[spec_operation("get_count", spec = "fixture.readonly_operation")]
    pub fn get_count(&self) -> i32 {
        self.count
    }
}

#[test]
fn readonly_method_preserves_state() {
    let counter = make_counter();
    assert_eq!(counter.get_count(), 42);
    assert_eq!(counter.count, 42);
}
