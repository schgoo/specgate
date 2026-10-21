use super::*;
use specgate::{ComponentId, ImplementationLabel};
use specgate_cli::discover::{RegistryId, RegistryVersion};
use std::path::PathBuf;

fn args(values: impl AsRef<[&'static str]>) -> Vec<String> {
    values.as_ref().iter().map(|value| (*value).to_string()).collect()
}

fn discover_flags(extra: &[&'static str]) -> Vec<String> {
    let mut flags = args([
        "--component",
        "fixture.add",
        "--registry-id",
        "registry",
        "--registry-version",
        "1",
        "--out",
        "registry.json",
    ]);
    flags.extend(extra.iter().map(|value| (*value).to_string()));
    flags
}

#[test]
fn parses_arguments() {
    assert_eq!(
        parse_capture(args(["--out", "capture", "--component", "fixture.add"])).unwrap(),
        CaptureArgs {
            binding: PathBuf::from("./specgate.yaml"),
            implementation: None,
            component: ComponentId::from("fixture.add"),
            out: Some(PathBuf::from("capture")),
            force: false,
        }
    );
    assert_eq!(
        parse_capture(args([
            "--binding",
            "other.yaml",
            "--impl",
            "rust",
            "--out",
            "capture",
            "--component",
            "fixture.add"
        ]))
        .unwrap(),
        CaptureArgs {
            binding: PathBuf::from("other.yaml"),
            implementation: Some(ImplementationLabel::from("rust")),
            component: ComponentId::from("fixture.add"),
            out: Some(PathBuf::from("capture")),
            force: false,
        }
    );
    assert_eq!(
        parse_discover(discover_flags(&["--binding", "binding.yaml", "--impl", "rust"])).unwrap(),
        DiscoverArgs {
            binding: "binding.yaml".into(),
            implementation: Some(ImplementationLabel::from("rust")),
            component: ComponentId::from("fixture.add"),
            registry_id: RegistryId::parse("registry").unwrap(),
            registry_version: RegistryVersion::parse("1").unwrap(),
            out: "registry.json".into()
        }
    );
    let defaulted = parse_discover(discover_flags(&[])).unwrap();
    assert_eq!(defaulted.binding, PathBuf::from("./specgate.yaml"));
    assert_eq!(defaulted.implementation, None);
    assert_eq!(
        parse_replay(args(["capture", "--impl", "rust", "--out", "candidate.json"])).unwrap(),
        ReplayArgs {
            capture_dir: PathBuf::from("capture"),
            binding: PathBuf::from("./specgate.yaml"),
            implementation: Some(ImplementationLabel::from("rust")),
            out: Some(PathBuf::from("candidate.json")),
            force: false,
        }
    );
    assert_eq!(
        parse_replay(args(["--binding", "candidate.yaml", "capture", "--out", "candidate.json"]))
            .unwrap()
            .binding,
        PathBuf::from("candidate.yaml")
    );
}

#[test]
fn removed_surfaces_are_rejected() {
    assert_eq!(
        parse_discover(discover_flags(&["--target", "rust"])).unwrap_err(),
        "unexpected argument '--target'"
    );
    assert_eq!(
        parse_discover(discover_flags(&["binding.yaml"])).unwrap_err(),
        "unexpected argument 'binding.yaml'"
    );
    assert_eq!(
        parse_capture(args(["binding.yaml", "--out", "capture", "--component", "fixture.add"])).unwrap_err(),
        "unexpected argument 'binding.yaml'"
    );
    assert_eq!(
        parse_capture(args(["--target", "rust", "--out", "capture", "--component", "fixture.add"])).unwrap_err(),
        "unexpected argument '--target'"
    );
    assert_eq!(
        parse_replay(args(["capture", "candidate.yaml", "--out", "candidate.json"])).unwrap_err(),
        "unexpected argument 'candidate.yaml'"
    );
    assert_eq!(
        parse_replay(args(["capture", "--target", "rust", "--out", "candidate.json"])).unwrap_err(),
        "unexpected argument '--target'"
    );
    assert_eq!(
        parse_capture(args(["--impl", "", "--out", "capture", "--component", "fixture.add"])).unwrap_err(),
        "--impl needs a non-empty implementation label"
    );
}

#[test]
fn parser_edges() {
    assert_eq!(parse_capture(args(["--unknown"])).unwrap_err(), "unexpected argument '--unknown'");
    assert_eq!(parse_discover(args(["--component"])).unwrap_err(), "--component needs an argument");
    assert_eq!(
        parse_discover(args(["--registry-id", "registry"])).unwrap_err(),
        "discover requires --component <id>"
    );
    assert_eq!(
        parse_capture(args(["--out", "first_out"])).unwrap_err(),
        "capture requires --component <id>"
    );
    assert_eq!(
        parse_capture(args(["--component", "fixture.add"])).unwrap(),
        CaptureArgs {
            binding: PathBuf::from("./specgate.yaml"),
            implementation: None,
            component: ComponentId::from("fixture.add"),
            out: None,
            force: false,
        }
    );
    assert!(parse_capture(args(["--component", "fixture.add", "--force"])).unwrap().force);
    assert_eq!(parse_capture(args(["--out"])).unwrap_err(), "--out needs an argument");
    assert_eq!(
        parse_replay(args(["capture", "extra", "--out", "candidate.json"])).unwrap_err(),
        "unexpected argument 'extra'"
    );
    assert_eq!(
        parse_replay(args(["--out", "candidate.json"])).unwrap_err(),
        "replay requires <capture-dir>"
    );
    let defaulted = parse_replay(args(["capture", "--force"])).unwrap();
    assert_eq!((defaulted.out, defaulted.force), (None, true));
    assert_eq!(
        parse_capture(args(["--component", "fixture.add", "--out", "first", "--out", "second"]))
            .unwrap()
            .out,
        Some(PathBuf::from("second"))
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
        cmd_discover(args(["--unknown"]), &mut output, &mut errors),
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
    assert_eq!(parse_inputs(args(["--import"])).unwrap_err(), "--import needs an argument");
    assert_eq!(parse_inputs(args(["--unknown"])).unwrap_err(), "unexpected argument '--unknown'");
    assert_eq!(
        parse_inputs(args(["root.json", "--import", "one.json", "two.json"])).unwrap(),
        (
            vec![PathBuf::from("root.json"), PathBuf::from("two.json")],
            vec![PathBuf::from("one.json")]
        )
    );
}

#[test]
fn compare_status() {
    assert_eq!(cmd_compare(args(["--registry"])), ExitCode::from(EXIT_USAGE));
    assert_eq!(
        cmd_compare(args([
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
        cmd_compare(args(["left.json", "right.json", "--import", "one.json"])),
        ExitCode::from(EXIT_USAGE)
    );
    assert_eq!(
        cmd_compare(args(["left.json", "right.json", "--unknown"])),
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
fn optional_outputs() {
    assert_eq!(parse_capture(args(["--component", "fixture.add"])).unwrap().out, None);
    assert_eq!(parse_replay(args(["capture"])).unwrap().out, None);
}

const USAGE_START: &str = "<!-- specgate-usage:start -->";
const USAGE_END: &str = "<!-- specgate-usage:end -->";

/// Returns the fenced text between the usage markers of a Markdown document.
fn documented_usage(markdown: &str) -> Option<String> {
    let markdown = markdown.replace("\r\n", "\n");
    let (_, rest) = markdown.split_once(USAGE_START)?;
    let (block, _) = rest.split_once(USAGE_END)?;
    let block = block.trim().strip_prefix("```text\n")?.strip_suffix("```")?;
    Some(block.trim_end_matches('\n').to_owned())
}

#[test]
fn usage_matches_cli_reference() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../docs/reference/cli.md");
    let markdown = std::fs::read_to_string(&path).unwrap();
    let documented = documented_usage(&markdown).expect("docs/reference/cli.md must fence the usage text between specgate-usage markers");
    assert_eq!(
        documented, USAGE,
        "docs/reference/cli.md usage block differs from `specgate --help`; copy the usage text into the marked block"
    );
}

#[test]
fn usage_snapshot_rejects_drift() {
    let good = format!("intro\n{USAGE_START}\n```text\n{USAGE}\n```\n{USAGE_END}\n");
    assert_eq!(documented_usage(&good).as_deref(), Some(USAGE));
    assert_eq!(documented_usage(&good.replace('\n', "\r\n")).as_deref(), Some(USAGE));

    let drifted = good.replace("validate bundle <capture-dir>", "validate bundle <dir>");
    assert_ne!(documented_usage(&drifted).as_deref(), Some(USAGE));
    assert_eq!(documented_usage("no markers"), None);
    assert_eq!(documented_usage(&format!("{USAGE_START}\n{USAGE}\n{USAGE_END}")), None);
}

#[test]
fn usage_is_printed_unchanged() {
    let mut stderr = Vec::new();
    assert_eq!(run(["--help"], &mut Vec::new(), &mut stderr), ExitCode::SUCCESS);
    assert_eq!(String::from_utf8(stderr).unwrap(), format!("{USAGE}\n"));
}
