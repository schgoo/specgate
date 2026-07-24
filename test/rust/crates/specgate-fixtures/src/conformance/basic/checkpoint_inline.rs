// Inline checkpoint during an operation.
use specgate::*;

#[spec_operation("process", spec = "fixture.checkpoint_inline")]
pub fn process(data: &str) -> String {
    let upper = data.to_uppercase();
    spec_trace!("after_upper", &upper);
    upper.trim().to_string()
}
