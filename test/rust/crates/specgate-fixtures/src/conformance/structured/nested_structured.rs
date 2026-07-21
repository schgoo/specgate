// List of maps — like OData entity properties.
use specgate::*;
use std::collections::BTreeMap;

#[spec_operation("get_properties")]
pub fn get_properties() -> Vec<BTreeMap<String, String>> {
    vec![
        {
            let mut m = BTreeMap::new();
            m.insert("name".to_string(), "ID".to_string());
            m.insert("type".to_string(), "Edm.Int32".to_string());
            m.insert("nullable".to_string(), "false".to_string());
            m
        },
        {
            let mut m = BTreeMap::new();
            m.insert("name".to_string(), "Name".to_string());
            m.insert("type".to_string(), "Edm.String".to_string());
            m.insert("nullable".to_string(), "true".to_string());
            m
        },
    ]
}
