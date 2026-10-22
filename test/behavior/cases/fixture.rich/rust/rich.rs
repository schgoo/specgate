use specgate::{SpecEvent, spec_operation};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.rich")]
pub struct Address {
    #[spec_event]
    pub street: String,
    #[spec_event]
    pub city: String,
}

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.rich")]
pub struct Person {
    #[spec_event]
    pub name: String,
    #[spec_event]
    pub address: Address,
    #[spec_event]
    pub tags: Vec<String>,
    #[spec_event]
    pub scores: BTreeMap<String, i32>,
    #[spec_event]
    pub aliases: BTreeSet<String>,
    #[spec_event]
    pub nickname: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.rich")]
pub enum Shape {
    Circle { radius: i32 },
    Rectangle { width: i32, height: i32 },
    Point,
}

#[spec_operation("describe", spec = "fixture.rich")]
pub fn describe(person: Person, fallback: Option<Shape>) -> Option<String> {
    tag_count(person.tags.clone());
    fallback.map(|shape| format!("{}:{shape:?}", person.name))
}

#[spec_operation("tag_count", spec = "fixture.rich")]
pub fn tag_count(tags: Vec<String>) -> i32 {
    i32::try_from(tags.len()).unwrap_or(i32::MAX)
}

#[test]
fn rich_values_project_natively() {
    let person = Person {
        name: "Ada".to_string(),
        address: Address {
            street: "1 Main".to_string(),
            city: "London".to_string(),
        },
        tags: vec!["engineer".to_string()],
        scores: BTreeMap::from([("quality".to_string(), 10)]),
        aliases: BTreeSet::from(["A".to_string()]),
        nickname: None,
    };
    assert_eq!(tag_count(person.tags.clone()), 1);
    assert_eq!(
        describe(person, Some(Shape::Point)),
        Some("Ada:Point".to_string())
    );
}
