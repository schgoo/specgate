use specgate::spec_operation;
use std::collections::BTreeMap;

#[spec_operation("get_entity_values", spec = "fixture.structured_map")]
pub fn get_entity_values(id: i32) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("ID".to_string(), id.to_string()),
        ("Name".to_string(), "Customer".to_string()),
        ("Email".to_string(), "cust@example.com".to_string()),
    ])
}

#[test]
fn map_output_has_stable_keys_and_values() {
    let entity = get_entity_values(12);
    assert_eq!(entity["ID"], "12");
    assert_eq!(entity["Email"], "cust@example.com");
}
