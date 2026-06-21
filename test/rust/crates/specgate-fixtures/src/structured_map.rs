// Operation returning a map.
use specgate::*;
use std::collections::BTreeMap;

#[spec_operation("get_entity_values")]
pub fn get_entity_values(id: i32) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("ID".to_string(), id.to_string());
    m.insert("Name".to_string(), "Customer".to_string());
    m.insert("Email".to_string(), "cust@example.com".to_string());
    m
}
