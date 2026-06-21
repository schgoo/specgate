// Fixture for testing assertion operators.
use specgate::*;
use std::collections::BTreeMap;

#[derive(SpecEvent)]
pub struct Product {
    #[spec_event(name = "product_name")]
    pub name: String,
    #[spec_event(name = "price")]
    pub price: i32,
    #[spec_event(name = "tags")]
    pub tags: Vec<String>,
    #[spec_event(name = "attributes")]
    pub attributes: BTreeMap<String, String>,
}

#[spec_operation("get_product")]
pub fn get_product() -> Product {
    let mut attrs = BTreeMap::new();
    attrs.insert("category".to_string(), "food".to_string());
    attrs.insert("origin".to_string(), "local".to_string());
    Product {
        name: "Milk".to_string(),
        price: 4,
        tags: vec!["dairy".to_string(), "organic".to_string(), "local".to_string()],
        attributes: attrs,
    }
}
