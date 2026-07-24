// Complex input/output types: structs, enums, lists, maps, and optionals.
use serde::{Deserialize, Serialize};
use specgate::*;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, SpecEvent)]
pub struct EnumMemberInput {
    #[spec_event]
    pub name: String,
    #[spec_event]
    pub value: String,
}

#[derive(Serialize, Deserialize, SpecEvent)]
pub struct Point {
    #[spec_event]
    pub x: i32,
    #[spec_event]
    pub y: i32,
}

#[derive(Serialize, Deserialize, SpecEvent)]
pub struct AppConfig {
    #[spec_event]
    pub name: String,
    #[spec_event]
    pub max_retries: i32,
    #[spec_event]
    pub verbose: bool,
}

#[derive(Serialize, Deserialize, SpecEvent)]
pub enum Shape {
    Circle { radius: i32 },
    Rectangle { width: i32, height: i32 },
    Point,
}

#[derive(Serialize, Deserialize, SpecEvent)]
pub struct Address {
    #[spec_event]
    pub street: String,
    #[spec_event]
    pub city: String,
}

#[derive(Serialize, Deserialize, SpecEvent)]
pub struct Person {
    #[spec_event]
    pub name: String,
    #[spec_event]
    pub age: i32,
    #[spec_event]
    pub address: Address,
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

#[spec_operation("create_enum_type", spec = "fixture.complex_inputs")]
pub fn create_enum_type(name: &str, members: Vec<EnumMemberInput>) -> String {
    spec_trace!("member_count", &(members.len() as i32));
    let first = members.first().map_or("".to_string(), |m| m.name.clone());
    spec_trace!("first_member", &first);
    name.to_string()
}

#[spec_operation("sum_points", spec = "fixture.complex_inputs")]
pub fn sum_points(points: Vec<Point>) -> Point {
    let x = points.iter().map(|p| p.x).sum();
    let y = points.iter().map(|p| p.y).sum();
    Point { x, y }
}

#[spec_operation("describe_config", spec = "fixture.complex_inputs")]
pub fn describe_config(config: AppConfig) -> String {
    // The `describe_config.config` input echo already witnesses that every
    // AppConfig field deserialized, so no derived observation is needed.
    config.name.clone()
}

#[spec_operation("area_of_shape", spec = "fixture.complex_inputs")]
pub fn area_of_shape(shape: Shape) -> i32 {
    match shape {
        Shape::Circle { radius } => (std::f64::consts::PI * (radius * radius) as f64) as i32,
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
    (0..count).map(|i| Point { x: i, y: i }).collect()
}

#[spec_operation("lookup", spec = "fixture.complex_inputs")]
pub fn lookup(table: HashMap<String, i32>, key: &str) -> i32 {
    *table.get(key).unwrap_or(&0)
}

#[spec_operation("invert_map", spec = "fixture.complex_inputs")]
pub fn invert_map(table: HashMap<String, i32>) -> HashMap<String, String> {
    table.into_iter().map(|(k, v)| (v.to_string(), k)).collect()
}

#[spec_operation("greet_optional", spec = "fixture.complex_inputs")]
pub fn greet_optional(name: Option<String>) -> String {
    match name {
        Some(n) => format!("Hello, {}!", n),
        None => "Hello, stranger!".to_string(),
    }
}

#[spec_operation("find_point", spec = "fixture.complex_inputs")]
pub fn find_point(points: Vec<Point>, target_x: i32) -> Option<Point> {
    points.into_iter().find(|p| p.x == target_x)
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
    // The `describe_person.person` input echo already witnesses the nested
    // Address (incl. city), so no derived `city` observation is needed.
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
