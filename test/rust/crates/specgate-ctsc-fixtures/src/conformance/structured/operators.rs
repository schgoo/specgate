use specgate::{SpecEvent, spec_operation};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.operators")]
pub struct Product {
    #[spec_event(name = "product_name")]
    pub name: String,
    #[spec_event]
    pub price: i32,
    #[spec_event]
    pub tags: Vec<String>,
    #[spec_event]
    pub attributes: BTreeMap<String, String>,
}

#[spec_operation("get_product", spec = "fixture.operators")]
pub fn get_product() -> Product {
    Product {
        name: "Milk".to_string(),
        price: 4,
        tags: vec![
            "dairy".to_string(),
            "organic".to_string(),
            "local".to_string(),
        ],
        attributes: BTreeMap::from([
            ("category".to_string(), "food".to_string()),
            ("origin".to_string(), "local".to_string()),
        ]),
    }
}

#[test]
fn matcher_vehicle_is_preserved_as_plain_structured_data() {
    let product = get_product();
    assert_eq!(product.name, "Milk");
    assert_eq!(product.price, 4);
    assert!(product.tags.contains(&"organic".to_string()));
    assert_eq!(product.attributes["origin"], "local");
}
