// Missing setup — operation references a setup that doesn't exist.
use specgate_annotations::*;

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
