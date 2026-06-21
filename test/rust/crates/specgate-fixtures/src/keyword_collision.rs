// Operation named "run" — tests that $run prefix prevents collision.
use specgate::*;

#[spec_operation("run")]
pub fn run(input: &str) -> String {
    format!("executed: {input}")
}
