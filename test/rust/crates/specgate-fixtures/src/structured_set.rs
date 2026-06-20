// Operation returning a set.
use specgate_annotations::*;
use std::collections::BTreeSet;

#[spec_operation("get_navigation_properties")]
pub fn get_navigation_properties() -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    s.insert("Orders".to_string());
    s.insert("Address".to_string());
    s.insert("Contacts".to_string());
    s
}
