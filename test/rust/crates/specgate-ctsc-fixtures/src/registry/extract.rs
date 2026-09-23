use specgate::{SpecEvent, spec_operation, spec_setup};
use std::collections::{BTreeMap, BTreeSet};

#[spec_operation("add", spec = "fixture.extract")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[spec_operation("fetch", spec = "fixture.extract")]
pub async fn fetch(url: String) -> String {
    format!("response from {url}")
}

#[spec_operation("divide", spec = "fixture.extract")]
pub fn divide(
    #[spec_input("numerator")] dividend: i32,
    #[spec_input("denominator")] divisor: i32,
) -> Result<i32, String> {
    if divisor == 0 {
        Err("divide by zero".to_string())
    } else {
        Ok(dividend / divisor)
    }
}

#[spec_operation("find", spec = "fixture.extract")]
pub fn find(items: Vec<i32>, target: i32) -> Option<i32> {
    items
        .iter()
        .position(|item| *item == target)
        .and_then(|index| i32::try_from(index).ok())
}

#[spec_operation("tally", spec = "fixture.extract")]
pub fn tally(values: Vec<i32>) -> BTreeMap<String, i32> {
    BTreeMap::from([
        (
            "count".to_string(),
            i32::try_from(values.len()).unwrap_or(i32::MAX),
        ),
        ("sum".to_string(), values.iter().sum()),
    ])
}

#[spec_operation("tags", spec = "fixture.extract")]
pub fn tags() -> BTreeSet<String> {
    BTreeSet::from(["alpha".to_string(), "beta".to_string()])
}

#[derive(Debug, Clone, PartialEq, SpecEvent)]
#[spec_component("fixture.extract")]
pub enum Shape {
    Circle { radius: f64 },
    Tag(String),
    Point,
}

#[spec_operation("classify", spec = "fixture.extract")]
pub fn classify(sides: i32) -> Shape {
    match sides {
        0 => Shape::Point,
        3 => Shape::Tag("triangle".to_string()),
        _ => Shape::Circle { radius: 1.0 },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.extract")]
pub struct Balance {
    #[spec_event]
    pub amount: i32,
    #[spec_event(name = "ccy")]
    pub currency: String,
    #[spec_event]
    pub money: Money,
}

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.extract")]
pub struct Money {
    #[spec_event]
    pub cents: i64,
}

#[spec_operation("make_balance", spec = "fixture.extract")]
pub fn make_balance(amount: i32, currency: String) -> Balance {
    Balance {
        amount,
        currency,
        money: Money { cents: 0 },
    }
}

#[spec_setup("double", spec = "fixture.extract")]
pub fn seed() -> i32 {
    21
}

#[spec_operation("double", spec = "fixture.extract")]
pub fn double(value: i32) -> i32 {
    value * 2
}

pub struct Scaler {
    factor: i32,
}

#[spec_setup("scale", spec = "fixture.extract")]
pub fn make_scaler(#[spec_input("factor")] multiplier: i32) -> Scaler {
    Scaler { factor: multiplier }
}

impl Scaler {
    #[spec_operation("scale", spec = "fixture.extract")]
    pub fn scale(&self, #[spec_input("value")] operand: i32) -> i32 {
        self.factor * operand
    }
}

#[test]
fn extraction_surfaces_have_concrete_behavior() {
    assert_eq!(add(2, 3), 5);
    assert_eq!(divide(12, 3), Ok(4));
    assert_eq!(divide(12, 0), Err("divide by zero".to_string()));
    assert_eq!(find(vec![4, 8, 15], 8), Some(1));
    assert_eq!(
        tally(vec![2, 3, 5]),
        BTreeMap::from([("count".to_string(), 3), ("sum".to_string(), 10)])
    );
    assert_eq!(
        tags(),
        BTreeSet::from(["alpha".to_string(), "beta".to_string()])
    );
    assert_eq!(classify(3), Shape::Tag("triangle".to_string()));
    assert_eq!(
        make_balance(25, "USD".to_string()),
        Balance {
            amount: 25,
            currency: "USD".to_string(),
            money: Money { cents: 0 }
        }
    );
    assert_eq!(double(seed()), 42);
    assert_eq!(make_scaler(6).scale(7), 42);
}
