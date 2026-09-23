use specgate::{SpecEvent, spec_operation, spec_setup, spec_trace};

#[derive(Debug, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.fallible_unit")]
pub struct UnitCounter {
    #[spec_event]
    pub count: i32,
}

#[spec_setup("advance", spec = "fixture.fallible_unit")]
pub fn make_counter(initial: i32) -> UnitCounter {
    UnitCounter { count: initial }
}

impl UnitCounter {
    #[spec_operation("advance", spec = "fixture.fallible_unit")]
    pub fn advance(&mut self) {
        self.count += 1;
        spec_trace!("count", self.count);
    }
}

pub mod strings {
    use specgate::spec_operation;

    pub const SPECIAL: &str =
        "nul:\0 backspace:\u{8} formfeed:\u{c} quote:\" slash:\\ cr:\r lf:\n tab:\t unicode:雪🙂";

    #[spec_operation("echo", spec = "fixture.strings")]
    pub fn echo(value: String) -> String {
        value
    }

    #[test]
    fn echoes_all_rust_string_escape_classes() {
        assert_eq!(echo(SPECIAL.to_string()), SPECIAL);
    }
}

#[spec_operation("fallible_void", spec = "fixture.fallible_unit")]
pub fn fallible_void(fail: bool) -> Result<(), String> {
    if fail {
        Err("failed".to_string())
    } else {
        Ok(())
    }
}

#[spec_operation("fallible_task", spec = "fixture.fallible_unit")]
pub async fn fallible_task(fail: bool) -> Result<(), String> {
    fallible_void(fail)
}

#[test]
fn setup_and_fallible_unit_paths_are_deterministic() {
    let mut counter = make_counter(7);
    counter.advance();
    assert_eq!(counter.count, 8);
    assert_eq!(fallible_void(false), Ok(()));
    assert_eq!(fallible_void(true), Err("failed".to_string()));
}
