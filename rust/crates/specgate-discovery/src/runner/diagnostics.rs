//! Failure-only subprocess context shared by discovery and CLI runners.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::process::{Command, Output};

const OUTPUT_CHARS: usize = 8_192;
const BUILD_ENVIRONMENT: [&str; 3] = ["CARGO_BUILD_BUILD_DIR", "CARGO_TARGET_DIR", "RUSTC_WORKSPACE_WRAPPER"];

/// Describe a failed subprocess without dumping its environment.
///
/// `inherited` reads environment values through the caller's concrete system
/// boundary. Explicit command overrides and removals take precedence. `None`
/// means the process could not start; a completed output preserves its actual
/// exit status, including signal termination, and bounded tails of both streams.
///
/// # Panics
/// Panics if `inherited` panics.
#[must_use]
#[expect(
    clippy::unnecessary_debug_formatting,
    reason = "quote and escape subprocess arguments and environment paths"
)]
pub fn process_diagnostic(command: &Command, output: Option<&Output>, inherited: impl Fn(&str) -> Option<OsString>) -> String {
    let mut diagnostic = format!("command: {:?}", command.get_program());
    for argument in command.get_args() {
        write!(diagnostic, " {argument:?}").expect("writing to a String cannot fail");
    }
    match command.get_current_dir() {
        Some(path) => write!(diagnostic, "\nworking directory: {}", path.display()),
        None => write!(diagnostic, "\nworking directory: inherited"),
    }
    .expect("writing to a String cannot fail");
    match output {
        Some(output) => write!(diagnostic, "\nstatus: {}", output.status),
        None => write!(diagnostic, "\nstatus: not started"),
    }
    .expect("writing to a String cannot fail");
    for name in BUILD_ENVIRONMENT {
        let configured = command.get_envs().find(|(key, _)| *key == name);
        match configured {
            Some((_, Some(value))) => write!(diagnostic, "\n{name}: {value:?} (override)"),
            Some((_, None)) => write!(diagnostic, "\n{name}: removed"),
            None => match inherited(name) {
                Some(value) => write!(diagnostic, "\n{name}: {value:?} (inherited)"),
                None => write!(diagnostic, "\n{name}: unset"),
            },
        }
        .expect("writing to a String cannot fail");
    }
    if let Some(output) = output {
        for (name, bytes) in [("stderr", &output.stderr), ("stdout", &output.stdout)] {
            if bytes.is_empty() {
                continue;
            }
            let text = String::from_utf8_lossy(bytes);
            let rendered = if name == "stdout" {
                text.lines()
                    .filter_map(|line| {
                        let message: serde_json::Value = serde_json::from_str(line).ok()?;
                        (message["reason"] == "compiler-message")
                            .then(|| message["message"]["rendered"].as_str().map(str::to_owned))
                            .flatten()
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                String::new()
            };
            let text = if rendered.is_empty() { text.as_ref() } else { &rendered };
            let start = text.char_indices().rev().nth(OUTPUT_CHARS - 1).map_or(0, |(index, _)| index);
            write!(diagnostic, "\n{name}:").expect("writing to a String cannot fail");
            if start > 0 {
                diagnostic.push_str("\n[earlier output truncated]");
            }
            write!(diagnostic, "\n{}", text[start..].trim()).expect("writing to a String cannot fail");
        }
    }
    diagnostic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_context_without_exposing_other_environment_values() {
        let mut command = Command::new("cargo with spaces");
        command
            .args(["run", "--manifest-path", "runner with spaces/Cargo.toml"])
            .current_dir("candidate with spaces")
            .env("CARGO_TARGET_DIR", "isolated-target")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .env("PRIVATE_TOKEN", "must-not-appear");
        let output = Output {
            status: super::super::system::test_status(101),
            stdout: b"compiler diagnostic on stdout".to_vec(),
            stderr: b"duplicate OPERATIONS".to_vec(),
        };
        let diagnostic = process_diagnostic(&command, Some(&output), |name| match name {
            "CARGO_BUILD_BUILD_DIR" => Some("shared-build".into()),
            "RUSTC_WORKSPACE_WRAPPER" => panic!("a removed variable must not be inherited"),
            _ => None,
        });
        for expected in [
            "\"cargo with spaces\"",
            "\"runner with spaces/Cargo.toml\"",
            "working directory: candidate with spaces",
            "101",
            "CARGO_BUILD_BUILD_DIR: \"shared-build\" (inherited)",
            "CARGO_TARGET_DIR: \"isolated-target\" (override)",
            "RUSTC_WORKSPACE_WRAPPER: removed",
            "duplicate OPERATIONS",
            "compiler diagnostic on stdout",
        ] {
            assert!(diagnostic.contains(expected), "{diagnostic}");
        }
        assert!(!diagnostic.contains("PRIVATE_TOKEN"));
        assert!(!diagnostic.contains("must-not-appear"));
    }

    #[test]
    fn output_tails_are_bounded_on_unicode_boundaries() {
        let command = Command::new("cargo");
        let output = Output {
            status: super::super::system::test_status(1),
            stdout: format!("discarded{}", "\u{1f980}".repeat(OUTPUT_CHARS)).into_bytes(),
            stderr: format!("discarded{}", "\u{e9}".repeat(OUTPUT_CHARS)).into_bytes(),
        };
        let diagnostic = process_diagnostic(&command, Some(&output), |_| None);
        assert!(diagnostic.contains("[earlier output truncated]"));
        assert!(!diagnostic.contains("discarded"));
        assert_eq!(diagnostic.matches('\u{1f980}').count(), OUTPUT_CHARS);
        assert_eq!(diagnostic.matches('\u{e9}').count(), OUTPUT_CHARS);
        let spawn = process_diagnostic(&command, None, |_| None);
        assert!(spawn.contains("status: not started"));
        assert!(!spawn.contains("stdout:"));
    }

    #[test]
    fn cargo_json_keeps_readable_compiler_diagnostics() {
        let output = Output {
            status: super::super::system::test_status(101),
            stdout: serde_json::to_vec(&serde_json::json!({
                "reason": "compiler-message",
                "message": { "rendered": "error: invalid input\n  --> source.rs:2:1\n" }
            }))
            .unwrap(),
            stderr: b"could not compile candidate".to_vec(),
        };
        let diagnostic = process_diagnostic(&Command::new("cargo"), Some(&output), |_| None);
        assert!(diagnostic.contains("error: invalid input\n  --> source.rs:2:1"));
        assert!(diagnostic.contains("could not compile candidate"));
    }

    #[test]
    #[cfg(unix)]
    fn signal_termination_preserves_the_native_status() {
        use std::os::unix::process::ExitStatusExt as _;

        let output = Output {
            status: std::process::ExitStatus::from_raw(9),
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
        let diagnostic = process_diagnostic(&Command::new("cargo"), Some(&output), |_| None);
        assert!(diagnostic.contains(&format!("status: {}", output.status)), "{diagnostic}");
        assert!(diagnostic.contains("signal: 9"), "{diagnostic}");
    }
}
