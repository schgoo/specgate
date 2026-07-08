use specgate_annotations::{spec_operation, spec_setup};
specgate_annotations::spec_component!("fixture.validation.setup_completeness");

pub struct Counter {
    pub n: i32,
}

#[spec_setup("increment")]
pub fn make_counter(initial: i32) -> Counter {
    Counter { n: initial }
}

impl Counter {
    #[spec_operation("increment")]
    pub fn increment(&mut self) -> i32 {
        self.n += 1;
        self.n
    }
}
