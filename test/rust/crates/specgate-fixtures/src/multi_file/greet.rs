// Operations split across multiple files — tests that the harness
// scans all .rs files, not just one.

use specgate::*;

#[spec_operation("greet")]
pub fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}
