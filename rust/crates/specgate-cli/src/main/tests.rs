use super::*;
use specgate::{ComponentId, TargetName};
use specgate_cli::discover::{RegistryId, RegistryVersion};
use std::path::PathBuf;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn parses_arguments() {
    assert_eq!(
        parse_capture(args(&["binding.yaml", "--out", "capture", "--component", "fixture.add"])).unwrap(),
        CaptureArgs {
            binding: PathBuf::from("binding.yaml"),
            target: TargetName::default(),
            component: ComponentId::from("fixture.add"),
            out: PathBuf::from("capture"),
        }
    );
    assert_eq!(
        parse_discover(args(&[
            "binding.yaml",
            "--target",
            "rust",
            "--component",
            "fixture.add",
            "--registry-id",
            "registry",
            "--registry-version",
            "1",
            "--out",
            "registry.json"
        ]))
        .unwrap(),
        DiscoverArgs {
            binding: "binding.yaml".into(),
            target: TargetName::from("rust"),
            component: ComponentId::from("fixture.add"),
            registry_id: RegistryId::parse("registry").unwrap(),
            registry_version: RegistryVersion::parse("1").unwrap(),
            out: "registry.json".into()
        }
    );
    assert_eq!(
        parse_replay(args(&["capture", "candidate.yaml", "--out", "candidate.json"])).unwrap(),
        ReplayArgs {
            capture_dir: PathBuf::from("capture"),
            binding: PathBuf::from("candidate.yaml"),
            target: TargetName::default(),
            out: PathBuf::from("candidate.json"),
        }
    );
}

#[test]
fn parser_edges() {
    assert_eq!(parse_capture(args(&["--unknown"])).unwrap_err(), "unexpected argument '--unknown'");
    assert_eq!(
        parse_discover(args(&["binding.yaml", "--component"])).unwrap_err(),
        "--component needs an argument"
    );
    assert_eq!(
        parse_discover(args(&["binding.yaml"])).unwrap_err(),
        "discover requires --component <id>"
    );
    assert_eq!(
        parse_capture(args(&["binding.yaml", "--out"])).unwrap_err(),
        "--out needs an argument"
    );
    assert_eq!(
        parse_replay(args(&["capture", "candidate.yaml", "extra", "--out", "candidate.json"])).unwrap_err(),
        "replay requires <capture-dir> and <candidate-binding.yaml>"
    );
    assert_eq!(
        parse_capture(args(&["binding.yaml", "--out", "first", "--out", "second"]))
            .unwrap()
            .out,
        PathBuf::from("second")
    );
}

#[test]
fn command_errors() {
    let mut output = Vec::new();
    let mut errors = Vec::new();
    assert_eq!(
        validate_to(Vec::<String>::new(), &mut output, &mut errors),
        ExitCode::from(EXIT_USAGE)
    );
    assert_eq!(cmd_compare(Vec::<String>::new()), ExitCode::from(EXIT_USAGE));
    assert_eq!(
        cmd_discover(args(&["--unknown"]), &mut output, &mut errors),
        ExitCode::from(EXIT_USAGE)
    );
    assert_eq!(
        cmd_capture(Vec::<String>::new(), &mut output, &mut errors),
        ExitCode::from(EXIT_USAGE)
    );
    assert_eq!(
        cmd_replay(Vec::<String>::new(), &mut output, &mut errors),
        ExitCode::from(EXIT_USAGE)
    );
    assert_eq!(parse_inputs(args(&["--import"])).unwrap_err(), "--import needs an argument");
    assert_eq!(parse_inputs(args(&["--unknown"])).unwrap_err(), "unexpected argument '--unknown'");
    assert_eq!(
        parse_inputs(args(&["root.json", "--import", "one.json", "two.json"])).unwrap(),
        (
            vec![PathBuf::from("root.json"), PathBuf::from("two.json")],
            vec![PathBuf::from("one.json")]
        )
    );
}

#[test]
fn compare_status() {
    assert_eq!(cmd_compare(args(&["--registry"])), ExitCode::from(EXIT_USAGE));
    assert_eq!(
        cmd_compare(args(&[
            "left.json",
            "right.json",
            "--registry",
            "one.json",
            "--registry",
            "two.json"
        ])),
        ExitCode::from(EXIT_USAGE)
    );
    assert_eq!(
        cmd_compare(args(&["left.json", "right.json", "--import", "one.json"])),
        ExitCode::from(EXIT_USAGE)
    );
    assert_eq!(
        cmd_compare(args(&["left.json", "right.json", "--unknown"])),
        ExitCode::from(EXIT_USAGE)
    );

    let trace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/ctsc/corpus/trace/valid/sequential.otlp.json")
        .into_os_string();
    assert_eq!(cmd_compare(vec![trace.clone(), trace]), ExitCode::SUCCESS);
}

#[cfg(windows)]
#[test]
fn compare_nonunicode() {
    use std::os::windows::ffi::OsStringExt;
    let path = std::ffi::OsString::from_wide(&[0xd800]);
    assert_eq!(cmd_compare(vec![path.clone(), path]), ExitCode::from(EXIT_FAILURE));
}

#[test]
fn required_outputs() {
    assert_eq!(parse_capture(args(&["binding.yaml"])).unwrap_err(), "capture requires --out <dir>");
    assert_eq!(
        parse_replay(args(&["capture", "candidate.yaml"])).unwrap_err(),
        "replay requires --out <candidate.otlp.json>"
    );
}
