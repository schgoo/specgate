use specgate::*;
specgate::spec_component!("test.value_multi");

#[spec_operation("passthrough")]
pub fn passthrough(id: i32) -> Value {
    Value::Integer(i64::from(id))
}
