use specgate::{SpecEvent, spec_operation, spec_setup};

specgate::spec_component!("fixture.cli");

pub const SPECIAL: &str = "nul:\0 backspace:\u{8} formfeed:\u{c} quote:\" slash:\\ cr:\r lf:\n tab:\t unicode:雪🙂";

#[spec_operation("add", spec = "fixture.cli.replay")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[spec_operation("echo", spec = "fixture.cli.replay")]
pub fn echo(value: String) -> String {
    value
}

#[derive(Debug, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.cli.setup")]
pub struct Counter {
    #[spec_event]
    pub count: i32,
}

#[spec_setup("increment", spec = "fixture.cli.setup")]
pub fn make_counter(initial: i32) -> Counter {
    Counter { count: initial }
}

impl Counter {
    #[spec_operation("increment", spec = "fixture.cli.setup")]
    pub fn increment(&mut self) {
        self.count += 1;
    }
}

#[spec_operation("used", spec = "fixture.cli.multiple")]
pub fn used(value: i32) -> i32 {
    value + 1
}

#[spec_operation("unexercised", spec = "fixture.cli.multiple")]
pub fn unexercised(value: i32) -> i32 {
    value - 1
}

#[spec_operation("never_called", spec = "fixture.cli.unused")]
pub fn never_called(value: i32) -> i32 {
    value
}

#[spec_operation("root", spec = "fixture.cli.profile_root")]
pub fn profile_root(value: i32) -> i32 {
    profile_bridge(value + 1)
}

#[spec_operation("bridge", spec = "fixture.cli.profile_bridge")]
pub fn profile_bridge(value: i32) -> i32 {
    profile_leaf(value + 1)
}

#[spec_operation("leaf", spec = "fixture.cli.profile_leaf")]
pub fn profile_leaf(value: i32) -> i32 {
    value + 1
}

/// A second top-level component that shares `leaf` as a nested callee.
///
/// Component capture of `fixture.cli.profile_root` must drop this whole
/// subtree, including its nested `leaf`, while leaving the root subtree intact.
#[spec_operation("sibling", spec = "fixture.cli.profile_sibling")]
pub fn profile_sibling(value: i32) -> i32 {
    profile_leaf(value + 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_two_and_three() {
        assert_eq!(add(2, 3), 5);
    }

    #[test]
    fn echoes_all_rust_string_escape_classes() {
        assert_eq!(echo(SPECIAL.to_string()), SPECIAL);
    }

    #[test]
    fn folds_setup_construction_input() {
        let mut counter = make_counter(4);
        counter.increment();
        assert_eq!(counter.count, 5);
    }

    #[test]
    fn captures_only_the_exercised_operation() {
        assert_eq!(used(4), 5);
    }

    #[test]
    fn captures_profile_ancestry_through_an_unselected_operation() {
        assert_eq!(profile_root(1), 4);
    }

    #[test]
    fn captures_two_top_level_components_in_one_scenario() {
        assert_eq!(profile_root(1), 4);
        assert_eq!(profile_sibling(1), 12);
    }

    #[test]
    fn deliberately_fails_for_strict_capture() {
        panic!("intentional focused fixture failure");
    }
}
