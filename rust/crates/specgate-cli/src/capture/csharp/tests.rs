#![cfg(test)]

//! Fake-driven tests of the C# runner and the sidecar protocol it settles with.
//! No process is spawned and no real file is touched.
use super::super::{reject_failures, unrecordable_warnings};
use super::*;
use specgate_discovery::registry::Registry;
use specgate_discovery::schema::normalize_registry;
use std::collections::{BTreeMap, BTreeSet};
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
        adapter: Adapter::XUnit,
    }
}

fn rendered(request: &ProcessRequest) -> String {
    format!("{request:?}")
}

/// The `MSBuild` property and item report of a tests project.
fn query_report(frameworks: &str, packages: &[&str]) -> String {
    let items: Vec<String> = packages.iter().map(|package| format!(r#"{{"Identity":"{package}"}}"#)).collect();
    format!(
        r#"{{"Properties":{{"TargetFramework":"","TargetFrameworks":"{frameworks}"}},"Items":{{"PackageReference":[{}]}}}}"#,
        items.join(",")
    )
}

fn push_query(execution: &Execution, packages: &[&str]) {
    execution.push(Ok(output(0, &query_report("net10.0", packages), "")));
}

const XUNIT: &[&str] = &["Microsoft.NET.Test.Sdk", "xunit.v3", "xunit.runner.visualstudio"];

#[test]
fn filters_escape_the_reserved_characters() {
    assert_eq!(exact_filter(Adapter::XUnit, "Ns.C.M"), "DisplayName=Ns.C.M");
    assert_eq!(
        exact_filter(Adapter::XUnit, r#"Ns.C.M(n: 2, s: "c(d)|e&f=g!h~i\j;k")"#),
        r"DisplayName=Ns.C.M\(n: 2%2C s: %22c\(d\)\|e\&f\=g\!h\~i\\j%3Bk%22\)"
    );
}

#[test]
fn each_adapter_filters_on_the_property_that_holds_its_listed_name() {
    assert_eq!(
        exact_filter(Adapter::XUnit, "AddsEveryCase(a: 1)"),
        r"DisplayName=AddsEveryCase\(a: 1\)"
    );
    // The MSTest and NUnit adapters list `Name` and implement no `DisplayName`.
    assert_eq!(
        exact_filter(Adapter::MSTest, "AddsEveryCase (1,2,3)"),
        r"Name=AddsEveryCase \(1%2C2%2C3\)"
    );
    assert_eq!(
        exact_filter(Adapter::NUnit, "AddsEveryCase(-4,4,0)"),
        r"Name=AddsEveryCase\(-4%2C4%2C0\)"
    );
    assert_eq!(exact_filter(Adapter::NUnit, "Plain"), "Name=Plain");
}

#[test]
fn a_literal_percent_escape_is_distinct_from_the_separator_it_spells() {
    for adapter in [Adapter::XUnit, Adapter::MSTest, Adapter::NUnit] {
        let property = adapter.filter_property();
        assert_eq!(exact_filter(adapter, "a,b"), format!("{property}=a%2Cb"));
        assert_eq!(exact_filter(adapter, "a%2Cb"), format!("{property}=a%252Cb"));
        assert_ne!(exact_filter(adapter, "a%2Cb"), exact_filter(adapter, "a,b"));
        assert_eq!(exact_filter(adapter, "100%"), format!("{property}=100%25"));
    }
}

#[test]
fn the_adapter_is_detected_from_the_package_references() {
    let project = Path::new("tests/T.csproj");
    let detect = |packages: &[&str]| Adapter::detect(packages.iter().copied(), project);
    assert_eq!(detect(XUNIT).unwrap(), Adapter::XUnit);
    assert_eq!(detect(&["xunit", "xunit.runner.visualstudio"]).unwrap(), Adapter::XUnit);
    assert_eq!(
        detect(&["Microsoft.NET.Test.Sdk", "MSTest.TestFramework", "MSTest.TestAdapter"]).unwrap(),
        Adapter::MSTest
    );
    assert_eq!(detect(&["MSTest"]).unwrap(), Adapter::MSTest);
    assert_eq!(detect(&["NUnit", "NUnit3TestAdapter"]).unwrap(), Adapter::NUnit);
    assert_eq!(detect(&["NUNIT"]).unwrap(), Adapter::NUnit, "package ids are case-insensitive");

    let unknown = detect(&["Microsoft.NET.Test.Sdk", "Expecto"]).unwrap_err();
    assert!(
        unknown.diagnostic().contains("no supported test framework"),
        "{}",
        unknown.diagnostic()
    );
    assert!(unknown.diagnostic().contains("tests/T.csproj"), "{}", unknown.diagnostic());
    let none = detect(&[]).unwrap_err();
    assert!(none.diagnostic().contains("no supported test framework"), "{}", none.diagnostic());

    let ambiguous = detect(&["xunit.v3", "NUnit"]).unwrap_err();
    assert!(
        ambiguous.diagnostic().contains("several test frameworks"),
        "{}",
        ambiguous.diagnostic()
    );
    assert!(ambiguous.diagnostic().contains("NUnit"), "{}", ambiguous.diagnostic());
    assert!(ambiguous.diagnostic().contains("xUnit"), "{}", ambiguous.diagnostic());
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
    assert_eq!(
        verify_selection(&system, log, "Ns.C.M").expect("a recorded failed result is an ordinary failed test"),
        "Failed"
    );

    system.seed_file(log, "<TestRun><UnitTestResult testName=\"x\" /></TestRun>");
    let unreadable = verify_selection(&system, log, "Ns.C.M").unwrap_err();
    assert!(unreadable.diagnostic().contains("'Ns.C.M'"), "{}", unreadable.diagnostic());
    assert!(unreadable.diagnostic().contains("outcome"), "{}", unreadable.diagnostic());
}

#[test]
fn only_a_passed_result_keeps_its_capture() {
    let settled = || settle_with(Some(sidecar("Ns.C.M", &empty_completion(), true)), &output(0, "", "")).unwrap();
    assert!(matches!(apply_outcome(settled(), "Passed", "Ns.C.M"), Settled::Captured(_)));

    for outcome in ["NotExecuted", "Skipped", "Inconclusive", "Aborted"] {
        let Settled::Failed(failure) = apply_outcome(settled(), outcome, "Ns.C.M") else {
            panic!("a {outcome} result must not publish a scenario");
        };
        assert_eq!(failure.reason, FailureReason::NotPassed, "{outcome}");
        assert!(failure.summary.contains(outcome), "{}", failure.summary);
    }

    let Settled::Failed(failed) = apply_outcome(settled(), "Failed", "Ns.C.M") else {
        panic!("a failed result must not publish a scenario");
    };
    assert_eq!(failed.reason, FailureReason::TestFailed);

    // Nothing recorded stays unrecorded, and an already failed test keeps its reason.
    assert!(matches!(apply_outcome(Settled::Skipped, "NotExecuted", "Ns.C.M"), Settled::Skipped));
    let already = settle_with(None, &output(1, "boom\n", "")).unwrap();
    let Settled::Failed(kept) = apply_outcome(already, "Failed", "Ns.C.M") else {
        panic!("a failed test stays failed");
    };
    assert_eq!(kept.reason, FailureReason::TestFailed);
}

#[test]
fn a_test_that_completes_an_operation_then_skips_is_not_captured_or_fails_strict_capture() {
    let settled = settle_with(Some(sidecar("Ns.C.M", &empty_completion(), true)), &output(0, "", "")).unwrap();
    let mut executed = ExecutedTests::default();
    executed.record(apply_outcome(settled, "NotExecuted", "Ns.C.M"));

    // Normal capture publishes nothing for it.
    assert!(executed.captures.is_empty());
    assert_eq!(executed.failures.len(), 1);
    // Strict capture refuses the whole batch.
    let strict = reject_failures(&executed).unwrap_err();
    assert!(strict.diagnostic().contains("Ns.C.M"), "{}", strict.diagnostic());
    assert!(strict.diagnostic().contains("NotExecuted"), "{}", strict.diagnostic());
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
    execution.push(Ok(output(0, &query_report("net8.0;net10.0", XUNIT), "")));
    let rejected = resolve_projects(&system, &execution, &declared, None, Path::new("scratch")).unwrap_err();
    assert!(rejected.diagnostic().contains("csharp.framework"), "{}", rejected.diagnostic());
    let query = rendered(&execution.requests()[0]);
    assert!(query.contains("msbuild"), "{query}");
    assert!(query.contains("-getProperty:TargetFramework"), "{query}");
    assert!(query.contains("-getProperty:TargetFrameworks"), "{query}");
    assert!(query.contains("-getItem:PackageReference"), "{query}");
}

#[test]
fn a_single_evaluated_framework_is_accepted_and_a_chosen_one_is_evaluated_under_it() {
    let system = CommandEnvironment::fake();
    let declared = [PathBuf::from("tests/Single.csproj")];
    system.seed_file(&declared[0], "<Project />");
    let execution = Execution::fake();
    execution.push(Ok(output(
        0,
        r#"{"Properties":{"TargetFramework":"net10.0","TargetFrameworks":""},"Items":{"PackageReference":[{"Identity":"NUnit"}]}}"#,
        "",
    )));
    let projects = resolve_projects(&system, &execution, &declared, None, Path::new("scratch")).unwrap();
    assert_eq!(projects[0].adapter, Adapter::NUnit);
    execution.push(Ok(output(0, &query_report("net10.0", &["MSTest.TestFramework"]), "")));
    resolve_projects(&system, &execution, &declared, None, Path::new("scratch")).unwrap();

    // A chosen framework lifts the single-framework rule, but the packages are
    // still read, under that framework, because they may be conditioned on it.
    execution.push(Ok(output(0, &query_report("net8.0;net10.0", XUNIT), "")));
    let projects = resolve_projects(&system, &execution, &declared, Some("net8.0"), Path::new("scratch")).unwrap();
    let query = rendered(execution.requests().last().unwrap());
    assert!(query.contains("-p:TargetFramework=net8.0"), "{query}");
    assert_eq!(projects[0].adapter, Adapter::XUnit);
    assert_eq!(projects[0].stem, "Single");
    assert_eq!(projects[0].artifacts, Path::new("scratch").join("p0").join("artifacts"));
}

#[test]
fn an_unknown_or_ambiguous_test_framework_stops_the_batch_before_building() {
    let system = CommandEnvironment::fake();
    let declared = [PathBuf::from("tests/Odd.csproj")];
    system.seed_file(&declared[0], "<Project />");
    let execution = Execution::fake();
    push_query(&execution, &["Microsoft.NET.Test.Sdk", "Expecto"]);
    let unknown = resolve_projects(&system, &execution, &declared, None, Path::new("scratch")).unwrap_err();
    assert!(
        unknown.diagnostic().contains("no supported test framework"),
        "{}",
        unknown.diagnostic()
    );
    push_query(&execution, &["xunit.v3", "MSTest.TestFramework"]);
    let ambiguous = resolve_projects(&system, &execution, &declared, Some("net8.0"), Path::new("scratch")).unwrap_err();
    assert!(
        ambiguous.diagnostic().contains("several test frameworks"),
        "{}",
        ambiguous.diagnostic()
    );
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
    assert_eq!(build(&system, &execution, &batch(None), &project).unwrap(), 3);
    let request = rendered(&execution.requests()[0]);
    assert!(request.contains("-p:SpecGateWeaveProject=src/Sources.csproj"), "{request}");
    assert!(request.contains("-p:CustomAfterMicrosoftCommonTargets="), "{request}");
    assert!(request.contains("-p:SpecGateWeaveReportPath="), "{request}");
    assert!(request.contains("-nodeReuse:false"), "{request}");

    execution.push(Ok(output(0, "", "")));
    system.seed_file(&project.report, "not json");
    let malformed = build(&system, &execution, &batch(None), &project).unwrap_err();
    assert!(malformed.diagnostic().contains("malformed"), "{}", malformed.diagnostic());

    execution.push(Ok(output(0, "", "")));
    system.remove_file(&project.report).unwrap();
    let absent = build(&system, &execution, &batch(None), &project).unwrap_err();
    assert!(absent.diagnostic().contains("did not report"), "{}", absent.diagnostic());

    execution.push(Ok(output(1, "error CS0001: nope\n", "")));
    let failed = build(&system, &execution, &batch(None), &project).unwrap_err();
    assert!(failed.diagnostic().contains("CS0001"), "{}", failed.diagnostic());
}

#[test]
fn a_weave_that_changed_nothing_fails_only_when_operations_exist() {
    let project = project();
    let nothing = require_woven(&batch(None), &project, 0, true).unwrap_err();
    assert!(nothing.diagnostic().contains("wove no method"), "{}", nothing.diagnostic());
    require_woven(&batch(None), &project, 0, false).expect("nothing is expected when discovery found no operations");
    require_woven(&batch(None), &project, 3, true).unwrap();
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

const COMPONENT: &str = "fixture.native";

/// The resolved C# implementation of [`COMPONENT`] over the given tests projects.
fn resolved(declared: &[PathBuf], framework: Option<&str>) -> ResolvedImplementation {
    ResolvedImplementation {
        binding_path: PathBuf::from("binding.yaml"),
        component: COMPONENT.into(),
        label: "csharp".into(),
        implementation: specgate_discovery::binding::Implementation::new(
            "src",
            Tests::Projects(declared.to_vec()),
            LanguageOptions::CSharp {
                framework: framework.map(str::to_owned),
            },
        )
        .expect("test implementation is valid"),
    }
}

/// What reflecting the woven assembly reports: one operation per component,
/// each in its own registry document, as the discovery tool publishes them.
fn reflected(resolved: &ResolvedImplementation, components: &[&str]) -> specgate_discovery::output::Batch {
    use specgate_discovery::output::Component;
    let mut batch = specgate_discovery::output::Batch {
        implementation: resolved.clone(),
        cargo_context: None,
        registry_json: Vec::new(),
        registries: Vec::new(),
        components: BTreeMap::new(),
        present_components: Vec::new(),
    };
    for (index, component) in components.iter().enumerate() {
        let json = format!(
            r#"{{"operations":[{{"name":"run","module_path":"Fixture","fn_name":"Run","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"()","fills":"","params":[],"component":"{component}"}}],"types":[]}}"#
        );
        let registry = Registry::parse(&json).expect("synthetic registry parses");
        let schema = normalize_registry(&registry, specgate_discovery::binding::Language::CSharp, *component);
        batch.registry_json.push(json);
        batch.registries.push(registry);
        batch.components.insert(
            (*component).into(),
            Component {
                registry_index: index,
                schema,
            },
        );
        batch.present_components.push((*component).into());
    }
    batch
}

/// Run a batch over `declared` against a fake build whose assembly declares `components`.
fn run_with(
    system: &CommandEnvironment,
    execution: &Execution,
    declared: &[PathBuf],
    framework: Option<&str>,
    components: &[&str],
) -> Result<(Discovered, ExecutedTests), FailureContext> {
    let resolved = resolved(declared, framework);
    let discovery = Discovery::fake();
    discovery.push_batch(Ok(reflected(&resolved, components)));
    let requests = [BundleRequest {
        component: COMPONENT.into(),
        out: PathBuf::from("out"),
        excluded_operations: BTreeSet::new(),
    }];
    run_batch(
        system,
        execution,
        &discovery,
        &Plan {
            resolved: &resolved,
            requests: &requests,
        },
        Path::new("scratch"),
    )
}

fn run(system: &CommandEnvironment, execution: &Execution, declared: &[PathBuf]) -> Result<ExecutedTests, FailureContext> {
    run_with(system, execution, declared, Some("net8.0"), &[COMPONENT]).map(|(_, executed)| executed)
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
    // Both projects are inspected before anything is built.
    push_query(&execution, XUNIT);
    push_query(&execution, XUNIT);
    for names in [["N.T1", "N.T2"].as_slice(), ["N.T1"].as_slice()] {
        let listing: String = names.iter().map(|name| format!("    {name}\n")).collect::<Vec<_>>().concat();
        execution.push(Ok(output(0, "", "")));
        execution.push(Ok(output(0, &format!("{LIST_BANNER}{listing}"), "")));
    }
    for _ in 0..3 {
        execution.push(Ok(output(0, "", "")));
    }

    let executed = run(&system, &execution, &declared).unwrap();
    assert!(executed.failures.is_empty(), "{:?}", executed.failures);
    assert_eq!(count_requests(&execution, "\"build\""), 2, "one build per project");
    assert_eq!(count_requests(&execution, "--list-tests"), 2, "one enumeration per project");
    assert_eq!(count_requests(&execution, "--filter"), 3, "one process per test");
    assert_eq!(
        count_requests(&execution, "tests/A.csproj"),
        5,
        "inspect, build, list, and two tests of A"
    );
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
    push_query(&execution, XUNIT);
    execution.push(Ok(output(0, "", "")));
    execution.push(Ok(output(0, &format!("{LIST_BANNER}    N.T1\n"), "")));
    execution.push(Ok(output(0, "", "")));

    run(&system, &execution, &declared).unwrap();
    assert_eq!(
        count_requests(&execution, "N.T1"),
        1,
        "the test process gets the filter and scenario name"
    );
    assert_eq!(count_requests(&execution, "A::"), 0, "a lone project adds no prefix");
}

#[test]
fn the_sources_are_compiled_once_and_every_component_reaches_publication() {
    let declared = [PathBuf::from("tests/A.csproj")];
    let system = world(&["tests/A.csproj"]);
    seed_results(&system, 1);
    let execution = Execution::fake();
    push_query(&execution, XUNIT);
    execution.push(Ok(output(0, "", "")));
    execution.push(Ok(output(0, &format!("{LIST_BANNER}    N.T1\n"), "")));
    execution.push(Ok(output(0, "", "")));

    // The assembly declares the bound component and one it calls into. Only one
    // discovery result is queued, so a second compilation or discovery panics.
    let (discovered, executed) = run_with(&system, &execution, &declared, Some("net8.0"), &[COMPONENT, "fixture.nested"]).unwrap();
    assert_eq!(
        count_requests(&execution, "\"build\""),
        1,
        "the weaving build is the only compilation of the sources"
    );
    assert_eq!(discovered.implementation.component.as_str(), COMPONENT);
    let components: Vec<&str> = executed.schemas.keys().map(String::as_str).collect();
    assert_eq!(components, ["fixture.native", "fixture.nested"]);
    assert!(executed.schemas.values().all(Result::is_ok), "{:?}", executed.schemas);
}

#[test]
fn a_component_the_assembly_does_not_declare_stops_the_batch_after_the_build() {
    let declared = [PathBuf::from("tests/A.csproj")];
    let system = world(&["tests/A.csproj"]);
    let execution = Execution::fake();
    push_query(&execution, XUNIT);
    execution.push(Ok(output(0, "", "")));
    execution.push(Ok(output(0, &format!("{LIST_BANNER}    N.T1\n"), "")));
    let error = run_with(&system, &execution, &declared, Some("net8.0"), &["fixture.other"]).unwrap_err();
    assert!(error.diagnostic().contains(COMPONENT), "{}", error.diagnostic());
    assert_eq!(
        count_requests(&execution, "--filter"),
        0,
        "no test may run without the bound component"
    );
}

#[test]
fn each_project_is_filtered_with_its_own_adapter() {
    let declared = [PathBuf::from("tests/A.csproj"), PathBuf::from("tests/B.csproj")];
    let system = world(&["tests/A.csproj", "tests/B.csproj"]);
    seed_results(&system, 2);
    let execution = Execution::fake();
    push_query(&execution, XUNIT);
    push_query(&execution, &["MSTest.TestFramework", "MSTest.TestAdapter"]);
    for name in ["N.T1", "N.T2 (1,2)"] {
        execution.push(Ok(output(0, "", "")));
        execution.push(Ok(output(0, &format!("{LIST_BANNER}    {name}\n"), "")));
    }
    for _ in 0..2 {
        execution.push(Ok(output(0, "", "")));
    }

    run(&system, &execution, &declared).unwrap();
    let filters: Vec<String> = execution
        .requests()
        .iter()
        .map(rendered)
        .filter(|request| request.contains("--filter"))
        .collect();
    assert_eq!(filters.len(), 2);
    assert!(filters[0].contains("DisplayName=N.T1"), "{}", filters[0]);
    // The request is rendered with `Debug`, which doubles each backslash.
    assert!(filters[1].contains(r"Name=N.T2 \\(1%2C2\\)"), "{}", filters[1]);
    assert!(!filters[1].contains("DisplayName"), "{}", filters[1]);
}

#[test]
fn duplicate_scenario_names_stop_the_batch_before_any_test_runs() {
    let system = world(&["tests/a/X.csproj", "tests/b/X.csproj"]);
    // Two projects with the same stem collide even after prefixing.
    let declared = [PathBuf::from("tests/a/X.csproj"), PathBuf::from("tests/b/X.csproj")];
    let execution = Execution::fake();
    push_query(&execution, XUNIT);
    push_query(&execution, XUNIT);
    for _ in 0..2 {
        execution.push(Ok(output(0, "", "")));
        execution.push(Ok(output(0, &format!("{LIST_BANNER}    N.T1\n"), "")));
    }
    let error = run(&system, &execution, &declared).unwrap_err();
    assert!(error.diagnostic().contains("listed more than once"), "{}", error.diagnostic());
    assert_eq!(count_requests(&execution, "--filter"), 0, "no test may run after a naming conflict");

    // A project listing the same name twice conflicts on its own. MSTest and
    // NUnit list a method name without its class, so two classes that share a
    // method name end up here instead of one filter running both.
    let single = [PathBuf::from("tests/a/X.csproj")];
    push_query(&execution, &["NUnit"]);
    execution.push(Ok(output(0, "", "")));
    execution.push(Ok(output(0, &format!("{LIST_BANNER}    N.T1\n    N.T1\n"), "")));
    let repeated = run(&system, &execution, &single).unwrap_err();
    assert!(repeated.diagnostic().contains("listed more than once"), "{}", repeated.diagnostic());
}

#[test]
fn an_unconfigured_framework_asks_msbuild_before_building_anything() {
    let declared = [PathBuf::from("tests/A.csproj")];
    let system = world(&["tests/A.csproj"]);
    let execution = Execution::fake();
    execution.push(Ok(output(0, &query_report("net8.0;net10.0", XUNIT), "")));
    let error = run_with(&system, &execution, &declared, None, &[COMPONENT]).unwrap_err();
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
fn an_unrecordable_value_drops_the_scenario_with_a_warning() {
    let fault = serde_json::json!({
        "kind": "fault", "order": 1, "time_unix_nano": 2, "fault_type": "specgate.unrecordable_value",
        "message": "parameter 'home' of 'fixture.native::fetch': a null was observed", "observer": "target",
    });
    let Settled::Failed(failure) = settle_with(Some(sidecar("Ns.C.M", &fault, true)), &output(0, "", "")).unwrap() else {
        panic!("an unrecordable value must drop its scenario");
    };
    assert_eq!(failure.reason, FailureReason::Unrecordable);
    assert!(failure.summary.contains("operation 'fetch'"), "{}", failure.summary);
    assert!(failure.summary.contains("parameter 'home'"), "{}", failure.summary);
    assert!(failure.summary.contains("optional<T>"), "{}", failure.summary);

    let executed = ExecutedTests {
        failures: vec![
            failure,
            FailedTest {
                scenario_name: "Ns.C.Other".to_string(),
                reason: FailureReason::TestFailed,
                summary: "exit code 1".to_string(),
            },
        ],
        ..ExecutedTests::default()
    };
    let warnings = unrecordable_warnings(&executed);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].starts_with("Ns.C.M: operation 'fetch'"), "{warnings:?}");
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
