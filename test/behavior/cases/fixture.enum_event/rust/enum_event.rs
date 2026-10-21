use specgate::{SpecEvent, spec_operation};

#[derive(Debug, Clone, PartialEq, SpecEvent)]
#[spec_component("fixture.enum_event")]
pub enum Shape {
    Circle { radius: f64 },
    Rectangle { width: f64, height: f64 },
    Tag(String),
    Point,
}

#[spec_operation("classify", spec = "fixture.enum_event")]
pub fn classify(sides: i32) -> Shape {
    match sides {
        0 => Shape::Point,
        1 => Shape::Circle { radius: 5.0 },
        3 => Shape::Tag("triangle".to_string()),
        4 => Shape::Rectangle {
            width: 3.0,
            height: 4.0,
        },
        _ => Shape::Point,
    }
}

#[test]
fn tagged_union_supports_unit_tuple_and_named_variants() {
    assert_eq!(classify(0), Shape::Point);
    assert_eq!(classify(3), Shape::Tag("triangle".to_string()));
    assert_eq!(
        classify(4),
        Shape::Rectangle {
            width: 3.0,
            height: 4.0
        }
    );
}
