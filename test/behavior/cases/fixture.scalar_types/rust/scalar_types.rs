use specgate::{SpecEvent, spec_operation};

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.scalar_types")]
pub enum ScalarValue {
    Integer(i64),
    Boolean(bool),
}

#[spec_operation("classify", spec = "fixture.scalar_types")]
pub fn classify(id: i64, active: bool) -> ScalarValue {
    if active {
        ScalarValue::Integer(id)
    } else {
        ScalarValue::Boolean(false)
    }
}

#[test]
fn i64_and_bool_scalars_are_capturable() {
    assert_eq!(
        classify(9_000_000_000, true),
        ScalarValue::Integer(9_000_000_000)
    );
    assert_eq!(classify(9_000_000_000, false), ScalarValue::Boolean(false));
}
