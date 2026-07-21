use specgate::*;

#[spec_operation("render", spec = "fixture.resolver_b")]
pub fn render() -> String {
    "from B".to_string()
}
