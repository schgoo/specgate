use specgate_annotations::{spec_operation, spec_setup};

#[spec_setup("make_request")]
fn make_request() -> ComputeRequest {
    ComputeRequest { x: 0, y: 0 }
}

#[spec_operation("compute")]
pub fn compute(req: ComputeRequest) -> i32 {
    req.x + req.y
}

pub struct ComputeRequest {
    pub x: i32,
    pub y: i32,
}
