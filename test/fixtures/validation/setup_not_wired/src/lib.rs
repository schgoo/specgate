use specgate_annotations::spec_operation;
specgate_annotations::spec_component!("fixture.validation.setup_not_wired");

pub struct Counter {
    pub n: i32,
}

impl Counter {
    #[spec_operation("increment")]
    pub fn increment(&mut self) -> i32 {
        self.n += 1;
        self.n
    }
}
