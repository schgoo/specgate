use specgate_annotations::{spec_operation, spec_setup};

#[spec_operation("add")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub struct Counter {
    pub n: i32,
}

#[spec_setup("increment")]
pub fn make_counter() -> Counter {
    Counter { n: 0 }
}

impl Counter {
    #[spec_operation("increment")]
    pub fn increment(&mut self) -> i32 {
        self.n += 1;
        self.n
    }
}

pub struct ComputeReq {
    pub x: i32,
    pub y: i32,
}

#[spec_operation("compute")]
pub fn compute(req: ComputeReq) -> i32 {
    req.x + req.y
}