use specgate::*;
specgate::spec_component!("test.value_escape_hatch");

#[spec_operation("passthrough")]
pub fn passthrough(id: i32) -> Value {
    Value::Integer(i64::from(id))
}
