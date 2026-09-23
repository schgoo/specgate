use specgate::{SpecEvent, Value, spec_operation};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, SpecEvent)]
#[spec_component("fixture.value")]
pub struct Record {
    #[spec_event]
    pub id: i32,
    #[spec_event]
    pub payload: Value,
    #[spec_event]
    pub history: Vec<Value>,
    #[spec_event]
    pub meta: BTreeMap<String, Value>,
    #[spec_event]
    pub tags: BTreeSet<Value>,
    #[spec_event]
    pub note: Option<Value>,
}

#[spec_operation("snapshot", spec = "fixture.value")]
pub fn snapshot() -> Record {
    Record {
        id: 1,
        payload: Value::Integer(42),
        history: vec![Value::Bool(true)],
        meta: BTreeMap::from([("kind".to_string(), Value::String("demo".to_string()))]),
        tags: BTreeSet::from([Value::String("alpha".to_string())]),
        note: Some(Value::Bool(false)),
    }
}

#[spec_operation("echo", spec = "fixture.value")]
pub fn echo(input: i32) -> Value {
    Value::Integer(i64::from(input))
}
