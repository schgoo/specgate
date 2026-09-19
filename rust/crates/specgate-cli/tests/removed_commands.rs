use std::process::Command;

#[test]
fn removed_commands_use_the_normal_unknown_command_error() {
    for command in ["run", "extract"] {
        let output = Command::new(env!("CARGO_BIN_EXE_specgate"))
            .arg(command)
            .output()
            .expect("run specgate");
        assert_eq!(output.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(&format!("unknown command '{command}'")),
            "unexpected stderr for {command}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
