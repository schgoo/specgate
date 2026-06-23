// Complex input/output types: structs, enums, lists, maps, and optionals.
use specgate::*;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, SpecEvent)]
pub struct EnumMemberInput {
    pub name: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, SpecEvent)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Serialize, Deserialize, SpecEvent)]
pub struct AppConfig {
    pub name: String,
    pub max_retries: i32,
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
    pub street: String,
    pub city: String,
}

#[derive(Serialize, Deserialize, SpecEvent)]
pub struct Person {
    pub name: String,
    pub age: i32,
    pub address: Address,
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

#[spec_operation("create_enum_type")]
pub fn create_enum_type(name: &str, members: Vec<EnumMemberInput>) -> String {
    emit_event("member_count", &format!("{}", members.len()));
    let first = members.first().map_or("".to_string(), |m| m.name.clone());
    emit_event("first_member", &first);
    name.to_string()
}

#[spec_operation("sum_points")]
pub fn sum_points(points: Vec<Point>) -> Point {
    let x = points.iter().map(|p| p.x).sum();
    let y = points.iter().map(|p| p.y).sum();
    Point { x, y }
}

#[spec_operation("describe_config")]
pub fn describe_config(config: AppConfig) -> String {
    let result = config.name.clone();
    // Emit $result before retry_info so the spec's expected order is satisfied.
    emit_event("$result", &result);
    emit_event(
        "retry_info",
        &format!("retries={}, verbose={}", config.max_retries, config.verbose),
    );
    result
}

#[spec_operation("area_of_shape")]
pub fn area_of_shape(shape: Shape) -> i32 {
    match shape {
        Shape::Circle { radius } => (std::f64::consts::PI * (radius * radius) as f64) as i32,
        Shape::Rectangle { width, height } => width * height,
        Shape::Point => 0,
    }
}

#[spec_operation("classify")]
pub fn classify(sides: i32) -> Shape {
    match sides {
        4 => Shape::Rectangle { width: 3, height: 4 },
        1 => Shape::Point,
        _ => Shape::Circle { radius: 5 },
    }
}

#[spec_operation("get_points_on_line")]
pub fn get_points_on_line(count: i32) -> Vec<Point> {
    (0..count).map(|i| Point { x: i, y: i }).collect()
}

#[spec_operation("lookup")]
pub fn lookup(table: HashMap<String, i32>, key: &str) -> i32 {
    *table.get(key).unwrap_or(&0)
}

#[spec_operation("invert_map")]
pub fn invert_map(table: HashMap<String, i32>) -> HashMap<String, String> {
    table.into_iter().map(|(k, v)| (v.to_string(), k)).collect()
}

#[spec_operation("greet_optional")]
pub fn greet_optional(name: Option<String>) -> String {
    match name {
        Some(n) => format!("Hello, {}!", n),
        None => "Hello, stranger!".to_string(),
    }
}

#[spec_operation("find_point")]
pub fn find_point(points: Vec<Point>, target_x: i32) -> Option<Point> {
    points.into_iter().find(|p| p.x == target_x)
}

#[spec_operation("find_shape")]
pub fn find_shape(sides: i32) -> Option<Shape> {
    match sides {
        1 => Some(Shape::Circle { radius: 5 }),
        0 => Some(Shape::Point),
        _ => None,
    }
}

#[spec_operation("describe_person")]
pub fn describe_person(person: Person) -> String {
    let result = format!("{}, age {}", person.name, person.age);
    // Emit $result before city so the spec's expected order is satisfied.
    emit_event("$result", &result);
    emit_event("city", &person.address.city);
    result
}

#[spec_operation("create_person")]
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
