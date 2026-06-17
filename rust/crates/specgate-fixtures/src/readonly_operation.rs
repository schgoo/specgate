// Read-only operation — no state changes.
use specgate_annotations::*;

#[spec_setup("make_counter")]
fn make_counter() -> Counter {
    Counter { count: 42 }
}

struct Counter {
    #[spec_event]
    count: i32,
}

impl Counter {
    #[spec_operation("get_count")]
    fn get_count(&self) -> i32 {
        self.count
    }
}
