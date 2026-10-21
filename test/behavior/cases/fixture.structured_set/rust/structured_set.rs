use specgate::spec_operation;
use std::collections::BTreeSet;

#[spec_operation("get_navigation_properties", spec = "fixture.structured_set")]
pub fn get_navigation_properties() -> BTreeSet<String> {
    BTreeSet::from([
        "Orders".to_string(),
        "Address".to_string(),
        "Contacts".to_string(),
    ])
}

#[test]
fn set_output_is_order_independent_and_deterministic() {
    assert_eq!(
        get_navigation_properties(),
        BTreeSet::from([
            "Address".to_string(),
            "Contacts".to_string(),
            "Orders".to_string()
        ])
    );
}
