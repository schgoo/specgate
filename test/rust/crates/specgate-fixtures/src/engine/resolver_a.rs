use specgate::*;

#[spec_operation("render", spec = "fixture.resolver_a")]
pub fn render() -> String {
    "from A".to_string()
}
