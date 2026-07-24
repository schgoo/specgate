use specgate::*;

#[spec_operation("farewell", spec = "fixture.multi_file")]
pub fn farewell(name: &str) -> String {
    format!("Goodbye, {name}!")
}
