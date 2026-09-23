use specgate::{SpecEvent, spec_operation};

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("comp.core")]
pub struct Widget {
    #[spec_event]
    pub id: i32,
    #[spec_event]
    pub label: String,
}

#[spec_operation("make_widget", spec = "comp.core")]
pub fn make_widget() -> Widget {
    Widget {
        id: 1,
        label: "widget".to_string(),
    }
}

#[spec_operation("assemble", spec = "comp.app")]
pub fn assemble() -> Widget {
    Widget {
        id: 2,
        label: "assembled".to_string(),
    }
}

#[test]
fn core_component_owns_widget_and_factory() {
    assert_eq!(
        make_widget(),
        Widget {
            id: 1,
            label: "widget".to_string()
        }
    );
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
