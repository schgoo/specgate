use specgate_annotations::spec_operation;

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
