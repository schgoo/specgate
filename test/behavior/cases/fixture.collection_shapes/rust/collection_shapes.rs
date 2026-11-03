use specgate::{SpecEvent, spec_operation};
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.collection_shapes")]
pub struct CollectionItem {
    #[spec_event]
    pub value: i32,
}

#[derive(Debug, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.collection_shapes")]
pub struct Grouping {
    #[spec_event]
    pub key: String,
    #[spec_event]
    pub items: Vec<i32>,
}

#[derive(Debug, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.collection_shapes")]
pub struct OptionalGrouping {
    #[spec_event]
    pub key: Option<String>,
    #[spec_event]
    pub items: Vec<i32>,
}

#[spec_operation("items", spec = "fixture.collection_shapes")]
pub fn items() -> Vec<CollectionItem> {
    vec![
        CollectionItem { value: 2 },
        CollectionItem { value: 1 },
        CollectionItem { value: 2 },
    ]
}

#[spec_operation("ordered_items", spec = "fixture.collection_shapes")]
pub fn ordered_items() -> Vec<i32> {
    vec![1, 2, 2]
}

#[spec_operation("ordered_map", spec = "fixture.collection_shapes")]
pub fn ordered_map() -> Vec<(String, i32)> {
    vec![("b".to_string(), 2), ("a".to_string(), 1)]
}

#[spec_operation("priorities", spec = "fixture.collection_shapes")]
pub fn priorities() -> Vec<(String, i32)> {
    vec![("later".to_string(), 7), ("first".to_string(), 1)]
}

#[spec_operation("lookup", spec = "fixture.collection_shapes")]
pub fn lookup() -> BTreeMap<String, Vec<i32>> {
    BTreeMap::from([("b".to_string(), vec![2, 2]), ("a".to_string(), vec![1])])
}

#[spec_operation("number_lookup", spec = "fixture.collection_shapes")]
pub fn number_lookup() -> Vec<(i32, Vec<String>)> {
    vec![
        (2, vec!["two".to_string(), "two".to_string()]),
        (1, vec!["one".to_string()]),
    ]
}

#[spec_operation("optional_lookup", spec = "fixture.collection_shapes")]
pub fn optional_lookup() -> Vec<(Option<String>, Vec<i32>)> {
    vec![(None, vec![2, 2]), (Some("a".to_string()), vec![1])]
}

#[spec_operation("grouping", spec = "fixture.collection_shapes")]
pub fn grouping() -> Grouping {
    Grouping {
        key: "group".to_string(),
        items: vec![2, 1, 2],
    }
}

#[spec_operation("optional_grouping", spec = "fixture.collection_shapes")]
pub fn optional_grouping() -> OptionalGrouping {
    OptionalGrouping {
        key: None,
        items: vec![2, 1, 2],
    }
}

#[spec_operation("bag_input", spec = "fixture.collection_shapes")]
pub fn bag_input(values: Vec<i32>) -> i32 {
    values.iter().sum()
}

#[spec_operation("set_input", spec = "fixture.collection_shapes")]
pub fn set_input(values: Vec<i32>) -> i32 {
    values.iter().sum()
}

#[spec_operation("ordered_map_input", spec = "fixture.collection_shapes")]
pub fn ordered_map_input(values: Vec<(String, i32)>) -> String {
    values
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(",")
}

#[spec_operation("number_map_input", spec = "fixture.collection_shapes")]
pub fn number_map_input(values: Vec<(i32, String)>) -> i32 {
    values.iter().map(|(key, _)| key).sum()
}

#[spec_operation("tuple_input", spec = "fixture.collection_shapes")]
pub fn tuple_input(value: (String, i32)) -> String {
    format!("{}{}", value.0, value.1)
}

#[spec_operation("nested_input", spec = "fixture.collection_shapes")]
pub fn nested_input(values: BTreeMap<String, Vec<(i32, i32)>>) -> i32 {
    values
        .values()
        .flatten()
        .map(|(left, right)| left + right)
        .sum()
}

#[test]
fn collections_keep_order_multiplicity_and_priority() {
    assert_eq!(
        items().iter().map(|item| item.value).collect::<Vec<_>>(),
        [2, 1, 2]
    );
    assert_eq!(ordered_items(), [1, 2, 2]);
    assert_eq!(ordered_map(), [("b".to_string(), 2), ("a".to_string(), 1)]);
    assert_eq!(
        priorities(),
        [("later".to_string(), 7), ("first".to_string(), 1)]
    );
}

#[test]
fn lookups_keep_keys_and_duplicate_values() {
    let values = lookup();
    assert_eq!(values["b"], [2, 2]);
    assert_eq!(values["a"], [1]);
    assert_eq!(
        number_lookup(),
        [
            (2, vec!["two".to_string(), "two".to_string()]),
            (1, vec!["one".to_string()])
        ]
    );
    assert_eq!(
        optional_lookup(),
        [(None, vec![2, 2]), (Some("a".to_string()), vec![1])]
    );
}

#[test]
fn groupings_keep_keys_and_multiplicity() {
    assert_eq!(
        grouping(),
        Grouping {
            key: "group".to_string(),
            items: vec![2, 1, 2]
        }
    );
    assert_eq!(
        optional_grouping(),
        OptionalGrouping {
            key: None,
            items: vec![2, 1, 2]
        }
    );
}

#[test]
fn collection_inputs_keep_values_and_associations() {
    assert_eq!(bag_input(vec![2, 1, 2]), 5);
    assert_eq!(set_input(vec![2, 1]), 3);
    assert_eq!(
        ordered_map_input(vec![("b".to_string(), 2), ("a".to_string(), 1)]),
        "b=2,a=1"
    );
    assert_eq!(
        number_map_input(vec![(2, "two".to_string()), (1, "one".to_string())]),
        3
    );
    assert_eq!(tuple_input(("item".to_string(), 7)), "item7");
    assert_eq!(
        nested_input(BTreeMap::from([(
            "group".to_string(),
            vec![(2, 3), (1, 4), (2, 3)]
        )])),
        15
    );
}
