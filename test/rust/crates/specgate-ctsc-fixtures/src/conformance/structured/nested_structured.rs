use specgate::spec_operation;
use std::collections::BTreeMap;

#[spec_operation("get_properties", spec = "fixture.nested_structured")]
pub fn get_properties() -> Vec<BTreeMap<String, String>> {
    vec![
        BTreeMap::from([
            ("name".to_string(), "ID".to_string()),
            ("type".to_string(), "Edm.Int32".to_string()),
            ("nullable".to_string(), "false".to_string()),
        ]),
        BTreeMap::from([
            ("name".to_string(), "Name".to_string()),
            ("type".to_string(), "Edm.String".to_string()),
            ("nullable".to_string(), "true".to_string()),
        ]),
    ]
}

#[test]
fn returns_a_list_of_nested_maps() {
    let properties = get_properties();
    assert_eq!(properties.len(), 2);
    assert_eq!(properties[0]["name"], "ID");
    assert_eq!(properties[1]["nullable"], "true");
}
