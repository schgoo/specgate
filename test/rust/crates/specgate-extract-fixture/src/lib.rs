//! A comprehensive annotated crate used to test deterministic spec extraction
//! (implementation -> spec). Every operation/type here isolates one or two of
//! the spec/annotation features that Part-A (schema-only, registry-based)
//! extraction must reproduce. The committed golden (`expected/extracted.spec.yaml`)
//! is the byte-exact target; `just extract-check` regression-guards it.
//!
//! Coverage map (all exercised here):
//!   * free-function operation, scalar inputs + `$result`        — `add`
//!   * async operation (`async: true`)                           — `fetch`
//!   * `#[spec_input]` parameter rename                          — `divide`, `scale`, `make_scaler`
//!   * `Result<T, E>` return                                     — `divide`
//!   * `Vec<T>` (List) input, `Option<T>` return                 — `find`
//!   * `BTreeMap<K, V>` (Map) return                             — `tally`
//!   * `BTreeSet<T>` (Set) return, no-input operation            — `tags`
//!   * `SpecEvent` enum (unit + named-field + tuple variants)    — `Shape` via `classify`
//!   * `SpecEvent` struct, nested type, `#[spec_event(name=…)]`  — `Balance`/`Money` via `make_balance`
//!   * `i64` scalar                                              — `Money.cents`
//!   * impl-method operation backed by a setup that fills the
//!     receiver; setup construction param becomes an input       — `scale` + `make_scaler`
//!   * setup that fills an operation param by type (param is
//!     omitted from `inputs:`)                                   — `double` + `seed`
//!
//! Out of scope for Part A (come from trace collection / body analysis, Part B):
//! test `cases:`, `#[spec_mock]` dependency I/O, `spec_trace!` checkpoints,
//! state-machine value ordering, `kind: command` targets.
use specgate::*;
use std::collections::{BTreeMap, BTreeSet};

/// Free function: scalar inputs, scalar `$result`.
#[spec_operation("add")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// Async operation -> `async: true`.
#[spec_operation("fetch")]
pub async fn fetch(url: String) -> String {
    format!("response from {url}")
}

/// `#[spec_input]` renames + `Result<T, E>` return.
#[spec_operation("divide")]
pub fn divide(#[spec_input("numerator")] a: i32, #[spec_input("denominator")] b: i32) -> Result<i32, String> {
    if b == 0 {
        Err("divide by zero".to_string())
    } else {
        Ok(a / b)
    }
}

/// `Vec<T>` (List) input + `Option<T>` return.
#[spec_operation("find")]
pub fn find(items: Vec<i32>, target: i32) -> Option<i32> {
    items.iter().position(|x| *x == target).map(|i| i32::try_from(i).unwrap_or(-1))
}

/// `BTreeMap<K, V>` (Map) return.
#[spec_operation("tally")]
pub fn tally(values: Vec<i32>) -> BTreeMap<String, i32> {
    let mut m = BTreeMap::new();
    m.insert("count".to_string(), i32::try_from(values.len()).unwrap_or(-1));
    m.insert("sum".to_string(), values.iter().sum());
    m
}

/// `BTreeSet<T>` (Set) return + no-input operation.
#[spec_operation("tags")]
pub fn tags() -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    s.insert("alpha".to_string());
    s.insert("beta".to_string());
    s
}

/// Operation returning a `SpecEvent` enum.
#[spec_operation("classify")]
pub fn classify(sides: i32) -> Shape {
    match sides {
        0 => Shape::Point,
        3 => Shape::Tag("triangle".to_string()),
        _ => Shape::Circle { radius: 1.0 },
    }
}

/// Operation returning a nested `SpecEvent` struct (with a renamed field).
#[spec_operation("make_balance")]
pub fn make_balance(amount: i32, currency: String) -> Balance {
    Balance {
        amount,
        currency,
        money: Money { cents: 0 },
    }
}

/// Setup that fills a primitive operation param **by type** (the `x` param is
/// setup-provided, so it is omitted from `double`'s `inputs:`).
#[spec_setup("double")]
pub fn seed() -> i32 {
    21
}

#[spec_operation("double")]
pub fn double(x: i32) -> i32 {
    x * 2
}

/// Setup that fills the receiver of an impl-method operation; the setup's own
/// construction param (`factor`) becomes an operation input.
#[spec_setup("scale")]
pub fn make_scaler(#[spec_input("factor")] f: i32) -> Scaler {
    Scaler { factor: f }
}

pub struct Scaler {
    factor: i32,
}

impl Scaler {
    /// Impl-method operation backed by `make_scaler`.
    #[spec_operation("scale")]
    pub fn scale(&self, #[spec_input("value")] v: i32) -> i32 {
        self.factor * v
    }
}

#[derive(SpecEvent)]
pub enum Shape {
    Circle { radius: f64 },
    Tag(String),
    Point,
}

#[derive(SpecEvent)]
pub struct Balance {
    #[spec_event]
    pub amount: i32,
    #[spec_event(name = "ccy")]
    pub currency: String,
    #[spec_event]
    pub money: Money,
}

#[derive(SpecEvent)]
pub struct Money {
    #[spec_event]
    pub cents: i64,
}
