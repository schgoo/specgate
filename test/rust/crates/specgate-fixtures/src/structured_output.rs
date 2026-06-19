// Operation returning structured data — list and map outputs.
use specgate_annotations::*;

#[derive(SpecEvent)]
pub struct EntityType {
    #[spec_event(name = "entity_name")]
    pub name: String,
    #[spec_event(name = "key_properties")]
    pub key_properties: Vec<String>,
    #[spec_event(name = "structural_properties")]
    pub structural_properties: Vec<String>,
}

#[spec_operation("resolve_entity")]
pub fn resolve_entity() -> EntityType {
    EntityType {
        name: "Customer".to_string(),
        key_properties: vec!["ID".to_string()],
        structural_properties: vec!["ID".to_string(), "Name".to_string(), "Email".to_string()],
    }
}
