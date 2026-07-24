// Operation named "run" — tests that $run prefix prevents collision.
use specgate::*;

#[spec_operation("run", spec = "fixture.keyword_collision")]
pub fn run(input: &str) -> String {
    format!("executed: {input}")
}
