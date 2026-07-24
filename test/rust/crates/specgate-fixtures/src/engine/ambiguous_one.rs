use specgate::*;

#[spec_operation("render_ambiguous", spec = "fixture.ambiguous_one")]
pub fn render_ambiguous() -> String {
    "one".to_string()
}
