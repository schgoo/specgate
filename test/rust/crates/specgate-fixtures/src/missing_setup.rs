// Missing setup — operation references a setup that doesn't exist.
use specgate_annotations::*;

#[derive(SpecEvent)]
pub struct Counter {
    #[spec_event]
    pub count: i32,
}

impl Counter {
    #[spec_operation("increment")]
    pub fn increment(&mut self) {
        self.count += 1;
    }
}
