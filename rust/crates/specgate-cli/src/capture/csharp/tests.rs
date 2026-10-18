#![cfg(test)]

//! Fake-driven tests of the C# runner and the sidecar protocol it settles with.
//! No process is spawned and no real file is touched.
use super::super::FailureReason;
use super::super::sidecar::Settled;
use super::*;
use std::process::{ExitStatus, Output};

fn status(code: i32) -> ExitStatus {
    #[cfg(unix)]
    {
        std::os::unix::process::ExitStatusExt::from_raw(code << 8)
    }
    #[cfg(windows)]
    {
        std::os::windows::process::ExitStatusExt::from_raw(u32::try_from(code).expect("non-negative exit code"))
    }
}

fn output(code: i32, stdout: &str, stderr: &str) -> Output {
    Output {
        status: status(code),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

fn batch(framework: Option<&str>) -> Batch<'_> {
    Batch {
        sources_project: PathBuf::from("src/Sources.csproj"),
        targets: PathBuf::from("weaver/SpecGateCapture.targets"),
        framework,
        results: PathBuf::from("scratch/results"),
    }
}

fn project() -> Project {
    Project {
        stem: "Fixture.Tests".to_string(),
        path: PathBuf::from("tests/Fixture.Tests.csproj"),
        artifacts: PathBuf::from("scratch/p0/artifacts"),
        report: PathBuf::from("scratch/p0/weave.json"),
    }
}

fn rendered(request: &ProcessRequest) -> String {
    format!("{request:?}")
}

#[test]
fn filters_escape_the_reserved_characters() {
    assert_eq!(exact_filter("Ns.C.M"), "DisplayName=Ns.C.M");
    assert_eq!(
        exact_filter(r#"Ns.C.M(n: 2, s: "c(d)|e&f=g!h~i\j;k")"#),
        r#"DisplayName=Ns.C.M\(n: 2%2C s: "c\(d\)\|e\&f\=g\!h\~i\\j%3Bk"\)"#
    );
}

#[test]
fn list_output_is_parsed_without_carriage_returns() {
    let execution = Execution::fake();
    execution.push(Ok(output(
        0,
        "banner\r\nThe following Tests are available:\r\n    Ns.C.Plain\r\n    Ns.C.Theory(n: 2, s: \"x\")\r\n    Ns.C.Theory(n: 3, s: \"y\")\r\nignored\r\n",
        "",
    )));
    let names = list_tests(&execution, &batch(None), &project()).unwrap();
    assert_eq!(names, ["Ns.C.Plain", "Ns.C.Theory(n: 2, s: \"x\")", "Ns.C.Theory(n: 3, s: \"y\")"]);
}

#[test]
fn list_output_failures_are_explicit() {
    let execution = Execution::fake();
    execution.push(Ok(output(0, "No tests here\n", "")));
    let missing = list_tests(&execution, &batch(None), &project()).unwrap_err();
    assert!(missing.diagnostic().contains("printed no test list"), "{}", missing.diagnostic());

    execution.push(Ok(output(1, "", "boom\n")));
    let failed = list_tests(&execution, &batch(None), &project()).unwrap_err();
    assert!(failed.diagnostic().contains("enumeration"), "{}", failed.diagnostic());
    assert!(failed.diagnostic().contains("boom"), "{}", failed.diagnostic());
}

#[test]
fn tool_requests_pin_the_environment_and_framework() {
    let plain = rendered(&tool_request(
        "test",
        &batch(None),
        &project(),
        [OsString::from("--no-build")],
        None,
    ));
    assert!(plain.contains("DOTNET_NOLOGO"), "{plain}");
    assert!(plain.contains("DOTNET_CLI_TELEMETRY_OPTOUT"), "{plain}");
    assert!(plain.contains("DOTNET_CLI_UI_LANGUAGE"), "{plain}");
    assert!(plain.contains("--artifacts-path"), "{plain}");
    assert!(!plain.contains("--framework"), "{plain}");
    assert!(!plain.contains("SPECGATE_NATIVE_CAPTURE"), "{plain}");

    let framework = rendered(&tool_request("test", &batch(Some("net8.0")), &project(), [], None));
    assert!(framework.contains("--framework"), "{framework}");
    assert!(framework.contains("net8.0"), "{framework}");

    let system = CommandEnvironment::fake();
    let sidecars = Sidecars::prepare(&system, "scratch").unwrap();
    let issued = sidecars.issue(&system, 0, "Ns.C.M").unwrap();
    let captured = rendered(&tool_request("test", &batch(None), &project(), [], Some(&issued)));
    assert!(captured.contains("SPECGATE_NATIVE_CAPTURE"), "{captured}");
}

#[test]
fn selection_must_be_exactly_one_test() {
    let system = CommandEnvironment::fake();
    let log = Path::new("scratch/results/00000000.trx");
    system.seed_file(log, "<TestRun><UnitTestResult outcome=\"Passed\" /></TestRun>");
    verify_selection(&system, log, "Ns.C.M", true).unwrap();

    system.seed_file(log, "<TestRun></TestRun>");
    let none = verify_selection(&system, log, "Ns.C.M", true).unwrap_err();
    assert!(none.diagnostic().contains("selected 0 tests"), "{}", none.diagnostic());

    system.seed_file(log, "<TestRun><UnitTestResult a=\"1\" /><UnitTestResult a=\"2\" /></TestRun>");
    let many = verify_selection(&system, log, "Ns.C.M", true).unwrap_err();
    assert!(many.diagnostic().contains("selected 2 tests"), "{}", many.diagnostic());

    let missing = Path::new("scratch/results/absent.trx");
    let unmatched = verify_selection(&system, missing, "Ns.C.M", true).unwrap_err();
    assert!(unmatched.diagnostic().contains("selected no test"), "{}", unmatched.diagnostic());
    verify_selection(&system, missing, "Ns.C.M", false).expect("a crashed host is a failed test, not a selection error");
}

#[test]
fn weaver_targets_are_located_explicitly() {
    let system = CommandEnvironment::fake();
    let absent = weaver_targets(&system).unwrap_err();
    assert!(absent.diagnostic().contains(TARGETS_FILE), "{}", absent.diagnostic());
    assert!(absent.diagnostic().contains(WEAVER_PATH_VAR), "{}", absent.diagnostic());

    system.seed_file(Path::new("fake-bin").join(TARGETS_FILE), "<Project />");
    assert_eq!(weaver_targets(&system).unwrap(), Path::new("fake-bin").join(TARGETS_FILE));

    system.seed_environment(WEAVER_PATH_VAR, "elsewhere");
    let overridden = weaver_targets(&system).unwrap_err();
    assert!(overridden.diagnostic().contains("elsewhere"), "{}", overridden.diagnostic());
    system.seed_file(Path::new("elsewhere").join(TARGETS_FILE), "<Project />");
    assert_eq!(weaver_targets(&system).unwrap(), Path::new("elsewhere").join(TARGETS_FILE));
}

#[test]
fn the_sources_project_is_the_first_project_file() {
    let system = CommandEnvironment::fake();
    let none = sources_project(&system, Path::new("src")).unwrap_err();
    assert!(none.diagnostic().contains("no .csproj"), "{}", none.diagnostic());
    system.seed_file("src/B.csproj", "<Project />");
    system.seed_file("src/A.csproj", "<Project />");
    assert_eq!(sources_project(&system, Path::new("src")).unwrap(), Path::new("src/A.csproj"));
}

#[test]
fn multi_targeted_projects_need_a_framework() {
    let system = CommandEnvironment::fake();
    let declared = [PathBuf::from("tests/Multi.csproj")];
    system.seed_file(
        &declared[0],
        "<Project><PropertyGroup><TargetFrameworks>net8.0;net10.0</TargetFrameworks></PropertyGroup></Project>",
    );
    let rejected = resolve_projects(&system, &declared, None, Path::new("scratch")).unwrap_err();
    assert!(rejected.diagnostic().contains("csharp.framework"), "{}", rejected.diagnostic());

    let projects = resolve_projects(&system, &declared, Some("net8.0"), Path::new("scratch")).unwrap();
    assert_eq!(projects[0].stem, "Multi");
    assert_eq!(projects[0].artifacts, Path::new("scratch").join("p0").join("artifacts"));
}

#[test]
fn the_build_proves_that_the_weaver_ran() {
    let system = CommandEnvironment::fake();
    let execution = Execution::fake();
    let project = project();

    execution.push(Ok(output(0, "", "")));
    system.seed_file(&project.report, r#"{"woven_method_count":3}"#);
    build(&system, &execution, &batch(None), &project, true).unwrap();
    let request = rendered(&execution.requests()[0]);
    assert!(request.contains("-p:SpecGateWeaveProject=src/Sources.csproj"), "{request}");
    assert!(request.contains("-p:CustomAfterMicrosoftCommonTargets="), "{request}");
    assert!(request.contains("-p:SpecGateWeaveReportPath="), "{request}");
    assert!(request.contains("-nodeReuse:false"), "{request}");

    execution.push(Ok(output(0, "", "")));
    system.seed_file(&project.report, r#"{"woven_method_count":0}"#);
    let nothing = build(&system, &execution, &batch(None), &project, true).unwrap_err();
    assert!(nothing.diagnostic().contains("wove no method"), "{}", nothing.diagnostic());
    execution.push(Ok(output(0, "", "")));
    build(&system, &execution, &batch(None), &project, false).expect("nothing is expected when discovery found no operations");

    execution.push(Ok(output(0, "", "")));
    system.seed_file(&project.report, "not json");
    let malformed = build(&system, &execution, &batch(None), &project, true).unwrap_err();
    assert!(malformed.diagnostic().contains("malformed"), "{}", malformed.diagnostic());

    execution.push(Ok(output(0, "", "")));
    system.remove_file(&project.report).unwrap();
    let absent = build(&system, &execution, &batch(None), &project, true).unwrap_err();
    assert!(absent.diagnostic().contains("did not report"), "{}", absent.diagnostic());

    execution.push(Ok(output(1, "error CS0001: nope\n", "")));
    let failed = build(&system, &execution, &batch(None), &project, true).unwrap_err();
    assert!(failed.diagnostic().contains("CS0001"), "{}", failed.diagnostic());
}

fn sidecar(scenario: &str, completion: &serde_json::Value, operations: bool) -> Vec<u8> {
    let operations = if operations {
        serde_json::json!([{
            "order": 0,
            "span_id": "1111111111111103",
            "parent_span_id": "1111111111111102",
            "component_id": "fixture.native",
            "operation_name": "fetch",
            "start_time_unix_nano": 1,
            "end_time_unix_nano": 2,
            "status": "ok",
            "inputs": {},
            "observations": [],
            "completion": completion,
        }])
    } else {
        serde_json::json!([])
    };
    serde_json::to_vec(&serde_json::json!({
        "trace_id": "11111111111111111111111111111111",
        "scenario_name": scenario,
        "run": {"span_id": "1111111111111101", "parent_span_id": null, "start_time_unix_nano": 0, "end_time_unix_nano": 4, "status": "ok"},
        "scenario": {"span_id": "1111111111111102", "parent_span_id": "1111111111111101", "start_time_unix_nano": 1, "end_time_unix_nano": 3, "status": "ok"},
        "operations": operations,
    }))
    .unwrap()
}

fn empty_completion() -> serde_json::Value {
    serde_json::json!({"kind": "empty", "order": 0, "time_unix_nano": 1})
}

fn settle_with(sidecar: Option<Vec<u8>>, result: &Output) -> Result<Settled, FailureContext> {
    let system = CommandEnvironment::fake();
    let sidecars = Sidecars::prepare(&system, "scratch").unwrap();
    let issued = sidecars.issue(&system, 0, "Ns.C.M").unwrap();
    if let Some(bytes) = sidecar {
        system.seed_file(Path::new("scratch").join("sidecars").join("00000000.json"), bytes);
    }
    issued.settle(&system, "Ns.C.M", result)
}

#[test]
fn a_finalized_sidecar_is_captured() {
    let settled = settle_with(Some(sidecar("Ns.C.M", &empty_completion(), true)), &output(0, "", "")).unwrap();
    assert!(matches!(settled, Settled::Captured(_)), "{settled:?}");
}

#[test]
fn a_passing_test_without_a_sidecar_is_skipped() {
    assert!(matches!(settle_with(None, &output(0, "", "")).unwrap(), Settled::Skipped));
    let empty = settle_with(Some(sidecar("Ns.C.M", &empty_completion(), false)), &output(0, "", "")).unwrap();
    assert!(matches!(empty, Settled::Skipped));
}

#[test]
fn a_failing_test_without_a_sidecar_is_a_failed_test() {
    let Settled::Failed(failure) = settle_with(None, &output(1, "assertion failed\n", "")).unwrap() else {
        panic!("a failing test must be reported as failed");
    };
    assert_eq!(failure.reason, FailureReason::TestFailed);
    assert_eq!(failure.scenario_name, "Ns.C.M");
}

#[test]
fn an_unfinalized_sidecar_is_reported() {
    let fault = serde_json::json!({
        "kind": "fault", "order": 1, "time_unix_nano": 2, "fault_type": "incomplete_capture",
        "message": "outstanding", "observer": "target",
    });
    let Settled::Failed(failure) = settle_with(Some(sidecar("Ns.C.M", &fault, true)), &output(0, "", "")).unwrap() else {
        panic!("an unfinalized recording must fail its test");
    };
    assert_eq!(failure.reason, FailureReason::Unfinalized);
}

#[test]
fn a_foreign_sidecar_stops_the_batch() {
    let foreign = settle_with(Some(sidecar("Other.Test", &empty_completion(), true)), &output(0, "", "")).unwrap_err();
    assert!(
        foreign.diagnostic().contains("does not carry the identity"),
        "{}",
        foreign.diagnostic()
    );
}

#[test]
fn a_malformed_sidecar_stops_the_batch() {
    let malformed = settle_with(Some(b"{ not json".to_vec()), &output(0, "", "")).unwrap_err();
    assert!(malformed.diagnostic().contains("failed to parse"), "{}", malformed.diagnostic());
    assert!(malformed.diagnostic().contains("Ns.C.M"), "{}", malformed.diagnostic());
}
