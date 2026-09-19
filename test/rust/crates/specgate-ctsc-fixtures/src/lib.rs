//! Focused fixtures for CTSC discovery, capture, and replay.

use specgate::{SpecEvent, spec_component, spec_operation, spec_setup, spec_trace};
use std::collections::{BTreeMap, BTreeSet};

spec_component!("fixture.ctsc");

/// Stateless replay smoke component.
pub mod stateless {
    use super::*;

    #[spec_operation("add", spec = "fixture.stateless_add")]
    pub fn add(a: i32, b: i32) -> i32 {
        a + b
    }

    #[test]
    fn add_two_and_three() {
        assert_eq!(add(2, 3), 5);
    }
}

/// Rich type and observation discovery component.
pub mod rich {
    use super::*;

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
        spec_trace!("tag_count", person.tags.len() as i32);
        fallback.map(|shape| format!("{}:{shape:?}", person.name))
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
        assert!(describe(person, Some(Shape::Point)).is_some());
    }
}

/// Setup-folding discovery component.
pub mod setup {
    use super::*;

    #[derive(Debug, SpecEvent)]
    #[spec_component("fixture.setup")]
    pub struct Counter {
        #[spec_event]
        pub count: i32,
    }

    #[spec_setup("increment", spec = "fixture.setup")]
    pub fn make_counter(initial: i32) -> Counter {
        Counter { count: initial }
    }

    impl Counter {
        #[spec_operation("increment", spec = "fixture.setup")]
        pub fn increment(&mut self) {
            self.count += 1;
        }
    }
}

/// Native fault coverage used by focused runtime tests.
pub mod faults {
    use super::*;

    #[spec_operation("explode", spec = "fixture.faults")]
    pub fn explode() -> i32 {
        panic!("fixture fault")
    }
}

/// Fallible unit and unscoped-setup parity component.
pub mod fallible_unit {
    use super::*;

    #[derive(Debug, SpecEvent)]
    #[spec_component("fixture.fallible_unit")]
    pub struct UnitCounter {
        #[spec_event]
        pub count: i32,
    }

    #[spec_setup("advance", spec = "fixture.fallible_unit")]
    pub fn make_counter(initial: i32) -> UnitCounter {
        UnitCounter { count: initial }
    }

    impl UnitCounter {
        #[spec_operation("advance", spec = "fixture.fallible_unit")]
        pub fn advance(&mut self) {
            self.count += 1;
        }
    }

    /// String escaping fixture for generated replay glue.
    pub mod strings {
        use super::*;

        pub const SPECIAL: &str = "nul:\0 backspace:\u{8} formfeed:\u{c} quote:\" slash:\\ cr:\r lf:\n tab:\t unicode:雪🙂";

        #[spec_operation("echo", spec = "fixture.strings")]
        pub fn echo(value: String) -> String {
            value
        }

        #[test]
        fn echoes_all_rust_string_escape_classes() {
            assert_eq!(echo(SPECIAL.to_string()), SPECIAL);
        }
    }

    #[spec_operation("fallible_void", spec = "fixture.fallible_unit")]
    pub fn fallible_void(fail: bool) -> Result<(), String> {
        if fail {
            Err("failed".to_string())
        } else {
            Ok(())
        }
    }

    #[spec_operation("fallible_task", spec = "fixture.fallible_unit")]
    pub async fn fallible_task(fail: bool) -> Result<(), String> {
        fallible_void(fail)
    }
}
