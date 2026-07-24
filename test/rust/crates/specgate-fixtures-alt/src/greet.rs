use specgate::*;

#[spec_operation("greet", spec = "fixture.target_selection")]
pub fn greet(name: &str) -> String {
    greet_impl(name)
}

#[spec_operation("greet", spec = "fixture.per_case_target")]
pub fn greet_per_case_target(name: &str) -> String {
    greet_impl(name)
}

fn greet_impl(name: &str) -> String {
    format!("Hello, {name}!")
}
