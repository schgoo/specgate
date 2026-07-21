use specgate::*;

#[spec_operation("render", spec = "fixture.resolver_conflict")]
pub fn render_one() -> String {
    "one".to_string()
}

#[spec_operation("render", spec = "fixture.resolver_conflict")]
pub fn render_two() -> String {
    "two".to_string()
}
