// Enum with SpecEvent — unit, tuple, and struct variants.
use specgate_annotations::*;

#[derive(SpecEvent)]
pub enum Shape {
    Circle { radius: f64 },
    Rectangle { width: f64, height: f64 },
    Point,
}

#[spec_operation("classify")]
pub fn classify(sides: i32) -> Shape {
    match sides {
        0 => Shape::Point,
        1 => Shape::Circle { radius: 5.0 },
        4 => Shape::Rectangle { width: 3.0, height: 4.0 },
        _ => Shape::Point,
    }
}
