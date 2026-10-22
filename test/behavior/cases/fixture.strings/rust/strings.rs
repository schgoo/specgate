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
