//! A small crate under test for coverage measurement. The spec exercises only
//! the positive branch of `classify`, leaving the negative branch and
//! `never_called` unexecuted, so a spec run yields deterministic *partial*
//! crate coverage.
use specgate::*;

#[spec_operation("classify")]
pub fn classify(n: i32) -> String {
    if n > 0 {
        positive_label()
    } else {
        negative_label()
    }
}

fn positive_label() -> String {
    "positive".to_string()
}

fn negative_label() -> String {
    "negative".to_string()
}

// Never invoked by any case, so its lines stay uncovered.
pub fn never_called() -> i32 {
    let mut total = 0;
    for i in 0..10 {
        total += i;
    }
    total
}
