use specgate::*;

#[spec_operation("farewell")]
pub fn farewell(name: &str) -> String {
    format!("Goodbye, {name}!")
}
