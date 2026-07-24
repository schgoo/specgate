use specgate::*;

#[spec_operation("render_ambiguous", spec = "fixture.ambiguous_two")]
pub fn render_ambiguous_alt() -> String {
    "two".to_string()
}
