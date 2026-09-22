use specgate::{SpecEvent, spec_operation};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.complex_inputs")]
pub struct EnumMemberInput {
    #[spec_event]
    pub name: String,
    #[spec_event]
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.complex_inputs")]
pub struct Point {
    #[spec_event]
    pub x: i32,
    #[spec_event]
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.complex_inputs")]
pub struct AppConfig {
    #[spec_event]
    pub name: String,
    #[spec_event]
    pub max_retries: i32,
    #[spec_event]
    pub verbose: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.complex_inputs")]
pub enum Shape {
    Circle { radius: i32 },
    Rectangle { width: i32, height: i32 },
    Point,
}

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.complex_inputs")]
pub struct Address {
    #[spec_event]
    pub street: String,
    #[spec_event]
    pub city: String,
}

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.complex_inputs")]
pub struct Person {
    #[spec_event]
    pub name: String,
    #[spec_event]
    pub age: i32,
    #[spec_event]
    pub address: Address,
}

#[spec_operation("create_enum_type", spec = "fixture.complex_inputs")]
pub fn create_enum_type(name: &str, members: Vec<EnumMemberInput>) -> String {
    member_count(&members);
    first_member(&members);
    name.to_string()
}

#[spec_operation("member_count", spec = "fixture.complex_inputs")]
pub fn member_count(members: &[EnumMemberInput]) -> i32 {
    i32::try_from(members.len()).unwrap_or(i32::MAX)
}

#[spec_operation("first_member", spec = "fixture.complex_inputs")]
pub fn first_member(members: &[EnumMemberInput]) -> String {
    members
        .first()
        .map_or_else(String::new, |member| member.name.clone())
}

#[spec_operation("sum_points", spec = "fixture.complex_inputs")]
pub fn sum_points(points: Vec<Point>) -> Point {
    Point {
        x: points.iter().map(|point| point.x).sum(),
        y: points.iter().map(|point| point.y).sum(),
    }
}

#[spec_operation("describe_config", spec = "fixture.complex_inputs")]
pub fn describe_config(config: AppConfig) -> String {
    config.name
}

#[spec_operation("area_of_shape", spec = "fixture.complex_inputs")]
pub fn area_of_shape(shape: Shape) -> i32 {
    match shape {
        Shape::Circle { radius } => (std::f64::consts::PI * f64::from(radius * radius)) as i32,
        Shape::Rectangle { width, height } => width * height,
        Shape::Point => 0,
    }
}

#[spec_operation("classify", spec = "fixture.complex_inputs")]
pub fn classify(sides: i32) -> Shape {
    match sides {
        4 => Shape::Rectangle {
            width: 3,
            height: 4,
        },
        1 => Shape::Point,
        _ => Shape::Circle { radius: 5 },
    }
}

#[spec_operation("get_points_on_line", spec = "fixture.complex_inputs")]
pub fn get_points_on_line(count: i32) -> Vec<Point> {
    (0..count)
        .map(|value| Point { x: value, y: value })
        .collect()
}

#[spec_operation("lookup", spec = "fixture.complex_inputs")]
pub fn lookup(table: BTreeMap<String, i32>, key: &str) -> i32 {
    table.get(key).copied().unwrap_or(0)
}

#[spec_operation("invert_map", spec = "fixture.complex_inputs")]
pub fn invert_map(table: BTreeMap<String, i32>) -> BTreeMap<String, String> {
    table
        .into_iter()
        .map(|(key, value)| (value.to_string(), key))
        .collect()
}

#[spec_operation("greet_optional", spec = "fixture.complex_inputs")]
pub fn greet_optional(name: Option<String>) -> String {
    name.map_or_else(
        || "Hello, stranger!".to_string(),
        |name| format!("Hello, {name}!"),
    )
}

#[spec_operation("find_point", spec = "fixture.complex_inputs")]
pub fn find_point(points: Vec<Point>, target_x: i32) -> Option<Point> {
    points.into_iter().find(|point| point.x == target_x)
}

#[spec_operation("find_shape", spec = "fixture.complex_inputs")]
pub fn find_shape(sides: i32) -> Option<Shape> {
    match sides {
        1 => Some(Shape::Circle { radius: 5 }),
        0 => Some(Shape::Point),
        _ => None,
    }
}

#[spec_operation("describe_person", spec = "fixture.complex_inputs")]
pub fn describe_person(person: Person) -> String {
    format!("{}, age {}", person.name, person.age)
}

#[spec_operation("create_person", spec = "fixture.complex_inputs")]
pub fn create_person(name: &str, age: i32, street: &str, city: &str) -> Person {
    Person {
        name: name.to_string(),
        age,
        address: Address {
            street: street.to_string(),
            city: city.to_string(),
        },
    }
}

#[test]
fn structured_inputs_and_outputs_use_real_values() {
    assert_eq!(
        create_enum_type(
            "Color",
            vec![
                EnumMemberInput {
                    name: "Red".to_string(),
                    value: "1".to_string(),
                },
                EnumMemberInput {
                    name: "Blue".to_string(),
                    value: "2".to_string(),
                },
            ],
        ),
        "Color"
    );
    assert_eq!(
        sum_points(vec![Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]),
        Point { x: 4, y: 6 }
    );
    assert_eq!(
        describe_config(AppConfig {
            name: "demo".to_string(),
            max_retries: 3,
            verbose: true,
        }),
        "demo"
    );
    assert_eq!(
        area_of_shape(Shape::Rectangle {
            width: 3,
            height: 4
        }),
        12
    );
    assert_eq!(
        classify(4),
        Shape::Rectangle {
            width: 3,
            height: 4
        }
    );
    assert_eq!(
        get_points_on_line(3),
        vec![
            Point { x: 0, y: 0 },
            Point { x: 1, y: 1 },
            Point { x: 2, y: 2 }
        ]
    );
    let table = BTreeMap::from([("alpha".to_string(), 7), ("beta".to_string(), 9)]);
    assert_eq!(lookup(table.clone(), "beta"), 9);
    assert_eq!(
        invert_map(table),
        BTreeMap::from([
            ("7".to_string(), "alpha".to_string()),
            ("9".to_string(), "beta".to_string())
        ])
    );
    assert_eq!(
        greet_optional(Some("Ada".to_string())),
        "Hello, Ada!".to_string()
    );
    assert_eq!(greet_optional(None), "Hello, stranger!".to_string());
    assert_eq!(
        find_point(vec![Point { x: 2, y: 8 }, Point { x: 4, y: 16 }], 4),
        Some(Point { x: 4, y: 16 })
    );
    assert_eq!(find_shape(0), Some(Shape::Point));
    let person = create_person("Grace", 37, "2 Main", "Arlington");
    assert_eq!(person.address.city, "Arlington");
    assert_eq!(describe_person(person), "Grace, age 37");
}
