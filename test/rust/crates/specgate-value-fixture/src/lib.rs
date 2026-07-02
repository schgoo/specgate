//! Fixture exercising the built-in `value` spec type: the runtime `Value` used
//! directly on a SpecEvent surface (scalar, list, map, set, and optional of
//! values) and as an operation result. Extraction must map `Value` -> `value`,
//! `Vec<Value>` -> `List<value>`, `BTreeMap<String, Value>` -> an inline `map`
//! of `value`, `BTreeSet<Value>` -> an inline `set` of `value`, and
//! `Option<Value>` -> `Option<value>`. The committed golden
//! (`expected/fixture.value.spec.yaml`) is the byte target.
use specgate::*;
use std::collections::{BTreeMap, BTreeSet};

spec_component!("fixture.value");

/// A SpecEvent struct carrying `Value` directly in scalar, list, map, set, and
/// optional form — covering every collection-of-value shape the extractor maps.
#[derive(SpecEvent)]
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

/// Operation returning a structured `Record`.
#[spec_operation("snapshot")]
pub fn snapshot() -> Record {
    let mut meta = BTreeMap::new();
    meta.insert("kind".to_string(), Value::String("demo".to_string()));
    let mut tags = BTreeSet::new();
    tags.insert(Value::String("alpha".to_string()));
    Record {
        id: 1,
        payload: Value::Integer(42),
        history: vec![Value::Bool(true)],
        meta,
        tags,
        note: Some(Value::Bool(false)),
    }
}

/// Operation whose `$result` is a bare `value`.
#[spec_operation("echo")]
pub fn echo(input: i32) -> Value {
    Value::Integer(i64::from(input))
}
