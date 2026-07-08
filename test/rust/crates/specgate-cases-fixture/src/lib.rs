//! Fixture for `specgate extract --cases`: case extraction from
//! existing tests. Free-function and method operations plus ordinary `#[test]`s
//! that exercise them. Running these tests under record mode captures one case
//! per test, named after the test, whose `expected:` is the full trace the
//! operation self-emits ($run, input echoes, $result). Setup-backed receivers
//! and side-effect setups are reconstructed into the case's `setup:` map from a
//! record-only echo; the setup itself stays invisible in `expected:`. The
//! committed golden (`expected/fixture.cases.spec.yaml`) is the byte target —
//! the schema merged with the captured cases.
use specgate::*;
use std::sync::atomic::{AtomicI32, Ordering};

spec_component!("fixture.cases");

/// Free function returning a scalar.
#[spec_operation("add")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// Free function taking and returning a string.
#[spec_operation("greet")]
pub fn greet(name: String) -> String {
    format!("hello, {name}")
}

/// A running total: a stateful receiver built by a setup with a construction
/// input, then mutated by a method operation.
#[derive(SpecEvent)]
pub struct Tally {
    #[spec_event]
    total: i32,
}

/// Setup: constructs the `adjust` receiver from a `start` construction input.
#[spec_setup("adjust")]
pub fn new_tally(start: i32) -> Tally {
    Tally { total: start }
}

impl Tally {
    /// Method operation: adds `amount` to the running total and returns it.
    #[spec_operation("adjust")]
    pub fn adjust(&mut self, amount: i32) -> i32 {
        self.total += amount;
        self.total
    }
}

static LIMIT: AtomicI32 = AtomicI32::new(0);

/// Side-effect setup: writes its `limit` construction input into global state.
#[spec_setup("check_limit")]
pub fn set_limit(limit: i32) {
    LIMIT.store(limit, Ordering::SeqCst);
}

/// Free-function operation reading global state established by its setup.
#[spec_operation("check_limit")]
pub fn check_limit() -> i32 {
    LIMIT.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Single invocation -> a simple case named `adds_two_and_three`.
    #[test]
    fn adds_two_and_three() {
        assert_eq!(add(2, 3), 5);
    }

    // Multiple independent invocations in one test -> a `steps:` case named
    // `adds_several`, one step per observed invocation.
    #[test]
    fn adds_several() {
        assert_eq!(add(2, 3), 5);
        assert_eq!(add(10, 20), 30);
    }

    // Single invocation of a string operation -> a simple case named `greets`.
    #[test]
    fn greets() {
        assert_eq!(greet("world".to_string()), "hello, world");
    }

    // Setup-backed receiver mutated over two method calls -> a `steps:` case
    // named `adjusts_from_ten` whose `setup: { start: 10 }` is recovered from
    // the setup's record-only echo, and whose steps carry the method args.
    #[test]
    fn adjusts_from_ten() {
        let mut t = new_tally(10);
        assert_eq!(t.adjust(5), 15);
        assert_eq!(t.adjust(-3), 12);
    }

    // Parameterized side-effect setup -> a simple case named
    // `checks_limit_of_seven` whose `setup: { limit: 7 }` global-state input is
    // recovered from the setup's record-only echo.
    #[test]
    fn checks_limit_of_seven() {
        set_limit(7);
        assert_eq!(check_limit(), 7);
    }
}
