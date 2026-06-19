use specgate_annotations::*;

#[spec_operation("greet")]
pub fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}
