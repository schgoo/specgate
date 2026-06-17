// Inline checkpoint during an operation.
use specgate_annotations::*;

#[spec_operation("process")]
fn process(data: &str) -> String {
    let upper = data.to_uppercase();
    spec_event!("after_upper", &upper);
    upper.trim().to_string()
}
