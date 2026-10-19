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
        r"DisplayName=Ns.C.M\(n: 2%2C s: %22c\(d\)\|e\&f\=g\!h\~i\\j%3Bk%22\)"
    );
}

#[test]
fn a_literal_percent_escape_is_distinct_from_the_separator_it_spells() {
    assert_eq!(exact_filter("a,b"), "DisplayName=a%2Cb");
    assert_eq!(exact_filter("a%2Cb"), "DisplayName=a%252Cb");
    assert_ne!(exact_filter("a%2Cb"), exact_filter("a,b"));
    assert_eq!(exact_filter("100%"), "DisplayName=100%25");
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
fn a_stray_line_mid_list_does_not_truncate_the_listing() {
    let execution = Execution::fake();
    execution.push(Ok(output(
        0,
        "The following Tests are available:\n    Ns.C.First\nwarning: something noisy\n    Ns.C.Second\n\n    Ns.C.Third\n",
        "",
    )));
    let names = list_tests(&execution, &batch(None), &project()).unwrap();
    assert_eq!(names, ["Ns.C.First", "Ns.C.Second", "Ns.C.Third"]);
}

#[test]
fn a_listing_without_tests_fails_closed() {
    let execution = Execution::fake();
    execution.push(Ok(output(0, "The following Tests are available:\nwarning: nothing\n", "")));
    let empty = list_tests(&execution, &batch(None), &project()).unwrap_err();
    assert!(empty.diagnostic().contains("listed no tests"), "{}", empty.diagnostic());
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
    assert!(!plain.contains("SPECGATE_NATIVE_CAPTURE\", Some"), "{plain}");
    assert!(
        plain.contains("SPECGATE_NATIVE_CAPTURE\", None"),
        "an inherited capture configuration must be removed from every tool request: {plain}"
    );

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
    verify_selection(&system, log, "Ns.C.M").unwrap();

    system.seed_file(log, "<TestRun></TestRun>");
    let none = verify_selection(&system, log, "Ns.C.M").unwrap_err();
    assert!(none.diagnostic().contains("selected 0 tests"), "{}", none.diagnostic());

    system.seed_file(log, "<TestRun><UnitTestResult a=\"1\" /><UnitTestResult a=\"2\" /></TestRun>");
    let many = verify_selection(&system, log, "Ns.C.M").unwrap_err();
    assert!(many.diagnostic().contains("selected 2 tests"), "{}", many.diagnostic());

    // A missing or truncated result is a batch error whether or not the process
    // succeeded; only a result that records a failed test is an ordinary failure.
    let missing = Path::new("scratch/results/absent.trx");
    let unmatched = verify_selection(&system, missing, "Ns.C.M").unwrap_err();
    assert!(unmatched.diagnostic().contains("'Ns.C.M'"), "{}", unmatched.diagnostic());
    assert!(
        unmatched.diagnostic().contains("no result was logged"),
        "{}",
        unmatched.diagnostic()
    );

    system.seed_file(log, "<TestRun><UnitTestResult outcome=\"Passed\" ");
    let truncated = verify_selection(&system, log, "Ns.C.M").unwrap_err();
    assert!(truncated.diagnostic().contains("'Ns.C.M'"), "{}", truncated.diagnostic());
    assert!(truncated.diagnostic().contains("incomplete"), "{}", truncated.diagnostic());

    system.seed_file(log, "<TestRun><UnitTestResult outcome=\"Failed\" /></TestRun>");
    verify_selection(&system, log, "Ns.C.M").expect("a recorded failed result is an ordinary failed test");
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
fn inherited_multi_targeting_is_found_in_the_evaluated_properties() {
    let system = CommandEnvironment::fake();
    let declared = [PathBuf::from("tests/Inherits.csproj")];
    // The project file itself names no framework list; Directory.Build.props supplies it.
    system.seed_file(&declared[0], "<Project Sdk=\"Microsoft.NET.Sdk\" />");
    let execution = Execution::fake();
    execution.push(Ok(output(
        0,
        "{\n  \"Properties\": {\n    \"TargetFramework\": \"\",\n    \"TargetFrameworks\": \"net8.0;net10.0\"\n  }\n}\n",
        "",
    )));
    let rejected = resolve_projects(&system, &execution, &declared, None, Path::new("scratch")).unwrap_err();
    assert!(rejected.diagnostic().contains("csharp.framework"), "{}", rejected.diagnostic());
    let query = rendered(&execution.requests()[0]);
    assert!(query.contains("msbuild"), "{query}");
    assert!(query.contains("-getProperty:TargetFramework"), "{query}");
    assert!(query.contains("-getProperty:TargetFrameworks"), "{query}");
}

#[test]
fn a_single_evaluated_framework_is_accepted_and_a_chosen_one_needs_no_query() {
    let system = CommandEnvironment::fake();
    let declared = [PathBuf::from("tests/Single.csproj")];
    system.seed_file(&declared[0], "<Project />");
    let execution = Execution::fake();
    execution.push(Ok(output(
        0,
        r#"{"Properties":{"TargetFramework":"net10.0","TargetFrameworks":""}}"#,
        "",
    )));
    resolve_projects(&system, &execution, &declared, None, Path::new("scratch")).unwrap();
    execution.push(Ok(output(
        0,
        r#"{"Properties":{"TargetFramework":"","TargetFrameworks":"net10.0"}}"#,
        "",
    )));
    resolve_projects(&system, &execution, &declared, None, Path::new("scratch")).unwrap();

    let before = execution.requests().len();
    let projects = resolve_projects(&system, &execution, &declared, Some("net8.0"), Path::new("scratch")).unwrap();
    assert_eq!(execution.requests().len(), before, "a chosen framework needs no property query");
    assert_eq!(projects[0].stem, "Single");
    assert_eq!(projects[0].artifacts, Path::new("scratch").join("p0").join("artifacts"));
}

#[test]
fn a_failed_property_query_stops_the_batch() {
    let system = CommandEnvironment::fake();
    let declared = [PathBuf::from("tests/Broken.csproj")];
    system.seed_file(&declared[0], "<Project />");
    let execution = Execution::fake();
    execution.push(Ok(output(1, "", "error MSB1003")));
    let failed = resolve_projects(&system, &execution, &declared, None, Path::new("scratch")).unwrap_err();
    assert!(failed.diagnostic().contains("MSB1003"), "{}", failed.diagnostic());
    execution.push(Ok(output(0, "not json", "")));
    let malformed = resolve_projects(&system, &execution, &declared, None, Path::new("scratch")).unwrap_err();
    assert!(malformed.diagnostic().contains("could not be read"), "{}", malformed.diagnostic());
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

const LIST_BANNER: &str = "The following Tests are available:\n";

/// A fake world with the sources project, the weaver, and the declared tests
/// projects, each reporting two woven methods.
fn world(projects: &[&str]) -> CommandEnvironment {
    let system = CommandEnvironment::fake();
    system.seed_file("src/Sources.csproj", "<Project />");
    system.seed_file(Path::new("fake-bin").join(TARGETS_FILE), "<Project />");
    for (index, project) in projects.iter().enumerate() {
        system.seed_file(project, "<Project />");
        system.seed_file(
            Path::new("scratch").join(format!("p{index}")).join("weave.json"),
            r#"{"woven_method_count":2}"#,
        );
    }
    system
}

fn plan(declared: &[PathBuf]) -> Plan<'_> {
    Plan {
        sources: Path::new("src"),
        declared,
        framework: Some("net8.0"),
        operations_discovered: true,
    }
}

fn seed_results(system: &CommandEnvironment, count: usize) {
    for index in 0..count {
        system.seed_file(
            Path::new("scratch").join("results").join(format!("{index:0INDEX_WIDTH$}.trx")),
            "<TestRun><UnitTestResult outcome=\"Passed\" /></TestRun>",
        );
    }
}

fn count_requests(execution: &Execution, needle: &str) -> usize {
    execution
        .requests()
        .iter()
        .filter(|request| rendered(request).contains(needle))
        .count()
}

#[test]
fn each_project_is_built_and_listed_once_and_its_scenarios_carry_the_project_name() {
    let declared = [PathBuf::from("tests/A.csproj"), PathBuf::from("tests/B.csproj")];
    let system = world(&["tests/A.csproj", "tests/B.csproj"]);
    seed_results(&system, 3);
    let execution = Execution::fake();
    for names in [["N.T1", "N.T2"].as_slice(), ["N.T1"].as_slice()] {
        let listing: String = names.iter().map(|name| format!("    {name}\n")).collect::<Vec<_>>().concat();
        execution.push(Ok(output(0, "", "")));
        execution.push(Ok(output(0, &format!("{LIST_BANNER}{listing}"), "")));
    }
    for _ in 0..3 {
        execution.push(Ok(output(0, "", "")));
    }

    let executed = run_batch(&system, &execution, &plan(&declared), Path::new("scratch")).unwrap();
    assert!(executed.failures.is_empty(), "{:?}", executed.failures);
    assert_eq!(count_requests(&execution, "\"build\""), 2, "one build per project");
    assert_eq!(count_requests(&execution, "--list-tests"), 2, "one enumeration per project");
    assert_eq!(count_requests(&execution, "--filter"), 3, "one process per test");
    assert_eq!(count_requests(&execution, "tests/A.csproj"), 4, "build, list, and two tests of A");
    for scenario in ["A::N.T1", "A::N.T2", "B::N.T1"] {
        assert_eq!(
            count_requests(&execution, scenario),
            1,
            "{scenario} is issued to exactly one test process"
        );
    }
}

#[test]
fn a_single_project_keeps_the_listed_names() {
    let declared = [PathBuf::from("tests/A.csproj")];
    let system = world(&["tests/A.csproj"]);
    seed_results(&system, 1);
    let execution = Execution::fake();
    execution.push(Ok(output(0, "", "")));
    execution.push(Ok(output(0, &format!("{LIST_BANNER}    N.T1\n"), "")));
    execution.push(Ok(output(0, "", "")));

    run_batch(&system, &execution, &plan(&declared), Path::new("scratch")).unwrap();
    assert_eq!(
        count_requests(&execution, "N.T1"),
        1,
        "the test process gets the filter and scenario name"
    );
    assert_eq!(count_requests(&execution, "A::"), 0, "a lone project adds no prefix");
}

#[test]
fn duplicate_scenario_names_stop_the_batch_before_any_test_runs() {
    let system = world(&["tests/a/X.csproj", "tests/b/X.csproj"]);
    // Two projects with the same stem collide even after prefixing.
    let declared = [PathBuf::from("tests/a/X.csproj"), PathBuf::from("tests/b/X.csproj")];
    let execution = Execution::fake();
    for _ in 0..2 {
        execution.push(Ok(output(0, "", "")));
        execution.push(Ok(output(0, &format!("{LIST_BANNER}    N.T1\n"), "")));
    }
    let error = run_batch(&system, &execution, &plan(&declared), Path::new("scratch")).unwrap_err();
    assert!(error.diagnostic().contains("listed more than once"), "{}", error.diagnostic());
    assert_eq!(count_requests(&execution, "--filter"), 0, "no test may run after a naming conflict");

    // A project listing the same display name twice conflicts on its own.
    let single = [PathBuf::from("tests/a/X.csproj")];
    execution.push(Ok(output(0, "", "")));
    execution.push(Ok(output(0, &format!("{LIST_BANNER}    N.T1\n    N.T1\n"), "")));
    let repeated = run_batch(&system, &execution, &plan(&single), Path::new("scratch")).unwrap_err();
    assert!(repeated.diagnostic().contains("listed more than once"), "{}", repeated.diagnostic());
}

#[test]
fn an_unconfigured_framework_asks_msbuild_before_building_anything() {
    let declared = [PathBuf::from("tests/A.csproj")];
    let system = world(&["tests/A.csproj"]);
    let execution = Execution::fake();
    execution.push(Ok(output(
        0,
        r#"{"Properties":{"TargetFramework":"","TargetFrameworks":"net8.0;net10.0"}}"#,
        "",
    )));
    let unframed = Plan {
        framework: None,
        ..plan(&declared)
    };
    let error = run_batch(&system, &execution, &unframed, Path::new("scratch")).unwrap_err();
    assert!(error.diagnostic().contains("csharp.framework"), "{}", error.diagnostic());
    assert_eq!(
        count_requests(&execution, "\"build\""),
        0,
        "nothing may be built for an ambiguous framework"
    );
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
