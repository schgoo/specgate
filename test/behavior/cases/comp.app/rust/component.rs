#[spec_operation("assemble", spec = "comp.app")]
pub fn assemble() -> Widget {
    Widget {
        id: 2,
        label: "assembled".to_string(),
    }
}

#[test]
fn app_component_returns_a_core_owned_type() {
    assert_eq!(
        assemble(),
        Widget {
            id: 2,
            label: "assembled".to_string()
        }
    );
}
