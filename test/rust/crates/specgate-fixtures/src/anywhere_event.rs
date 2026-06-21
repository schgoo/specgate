// Fixture used by anywhere_event.spec.yaml — make_counter + increment_twice
// with a single count field, so $anywhere can match count=0/1/2.
use specgate::*;

#[spec_setup("make_counter")]
pub fn make_counter() -> Counter {
    Counter { count: 0 }
}

#[derive(SpecEvent)]
pub struct Counter {
    #[spec_event]
    pub count: i32,
}

impl Counter {
    #[spec_operation("increment_twice")]
    pub fn increment_twice(&mut self) {
        self.count += 1;
        self.count += 1;
    }
}
