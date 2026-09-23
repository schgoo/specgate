use specgate::{SpecEvent, spec_operation};

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.type_exact")]
pub struct Scalars {
    #[spec_event]
    pub count: i32,
    #[spec_event]
    pub enabled: bool,
    #[spec_event]
    pub code: String,
}

#[spec_operation("get_scalars", spec = "fixture.type_exact")]
pub fn get_scalars() -> Scalars {
    Scalars {
        count: 5,
        enabled: true,
        code: "7".to_string(),
    }
}

#[test]
fn numeric_boolean_and_numeric_string_remain_distinct() {
    let scalars = get_scalars();
    assert_eq!(scalars.count, 5);
    assert!(scalars.enabled);
    assert_eq!(scalars.code, "7");
}
