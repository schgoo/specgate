// This fixture depends ONLY on the `specgate` umbrella crate (not
// specgate-annotations directly). It verifies that the proc macro
// expansion path ::specgate_annotations::__rt::... resolves through
// the umbrella's `pub extern crate specgate_annotations` re-export.

use specgate::{spec_operation, emit_event};

#[spec_operation("echo")]
pub fn echo(msg: &str) -> String {
    emit_event("input", msg);
    msg.to_string()
}
