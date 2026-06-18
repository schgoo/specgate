// Read-only operation — no state changes.
use specgate_annotations::*;

#[spec_setup("make_counter")]
pub fn make_counter() -> Counter {
    Counter { count: 42 }
}

#[derive(SpecEvent)]
pub struct Counter {
    #[spec_event]
    pub count: i32,
}

impl Counter {
    #[spec_operation("get_count")]
    pub fn get_count(&self) -> i32 {
        self.count
    }
}
