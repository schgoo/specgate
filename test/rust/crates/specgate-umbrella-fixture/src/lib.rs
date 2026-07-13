// This fixture depends ONLY on the `specgate` umbrella crate (not
// specgate-annotations directly). It verifies that the proc macro
// expansion path ::specgate_annotations::__rt::... resolves through
// the umbrella's `pub extern crate specgate_annotations` re-export.

use specgate::{spec_operation, spec_trace};

specgate::spec_component!("fixture.umbrella");

#[spec_operation("echo")]
pub fn echo(msg: &str) -> String {
    spec_trace!("input", msg.to_string());
    msg.to_string()
}
