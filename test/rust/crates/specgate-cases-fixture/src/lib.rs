//! Fixture for `specgate extract --cases`: case extraction from
//! existing tests. Free-function operations plus ordinary `#[test]`s that
//! exercise them. Running these tests under record mode captures one case per
//! test, named after the test, whose `expected:` is the full trace the
//! operation self-emits ($run, input echoes, $result). The committed golden
//! (`expected/fixture.cases.spec.yaml`) is the byte target — the schema
//! merged with the captured cases.
//!
//! v1 scope is FREE FUNCTIONS ONLY, so both operations here are free functions.
use specgate::*;

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
}
