use specgate::{SpecEvent, spec_operation};

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.tuple_values")]
pub enum TupleValue {
    Values(i32, String, bool),
}

#[spec_operation("summarize", spec = "fixture.tuple_values")]
pub fn summarize(count: i32, label: String, active: bool) -> TupleValue {
    TupleValue::Values(count, label, active)
}

#[test]
fn tuple_inputs_produce_a_tuple_output() {
    assert_eq!(
        summarize(3, "ready".to_string(), true),
        TupleValue::Values(3, "ready".to_string(), true)
    );
}
