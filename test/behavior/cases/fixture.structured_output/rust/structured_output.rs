use specgate::{SpecEvent, spec_operation};

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.structured_output")]
pub struct EntityType {
    #[spec_event(name = "entity_name")]
    pub name: String,
    #[spec_event]
    pub key_properties: Vec<String>,
    #[spec_event]
    pub structural_properties: Vec<String>,
}

#[spec_operation("resolve_entity", spec = "fixture.structured_output")]
pub fn resolve_entity() -> EntityType {
    EntityType {
        name: "Customer".to_string(),
        key_properties: vec!["ID".to_string()],
        structural_properties: vec!["ID".to_string(), "Name".to_string(), "Email".to_string()],
    }
}

#[test]
fn structured_output_projects_renamed_and_list_fields() {
    let entity = resolve_entity();
    assert_eq!(entity.name, "Customer");
    assert_eq!(entity.key_properties, vec!["ID"]);
    assert_eq!(entity.structural_properties.len(), 3);
}
