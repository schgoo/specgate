//! C# capture runner: build the native test project with the capture weaver,
//! enumerate its tests with `dotnet test --list-tests`, and run each test in
//! its own `dotnet test` process.
//!
//! The decisions behind this runner are recorded in
//! `docs/decisions/csharp-capture-cli.md`. In short: the sources project is
//! woven during the build of each tests project through an `MSBuild` targets file
//! injected from the command line, so no project file is edited; every
//! enumerated test name is one scenario, selected with the filter property of
//! the project's detected test framework (xUnit, `MSTest`, or `NUnit`); and a
//! test-result log proves that each exact-name filter selected exactly one test.
use super::facade::BundleRequest;
use super::sidecar::{Issued, Settled, Sidecars};
use super::{
    CommandEnvironment, ComponentSchemas, Discovered, Discovery, ExecutedTests, Execution, FailedTest, FailureContext, FailureReason,
    INDEX_WIDTH, Path, PathBuf, ProcessRequest, ProcessRequestBuilder, validate_setups,
};
use specgate_discovery::binding::{LanguageOptions, ResolvedImplementation, Tests};
use specgate_discovery::output::SchemaLookup;
use std::ffi::OsString;

// The weaver ships these files in one directory beside the `specgate`
// executable. `SPECGATE_WEAVER_PATH` overrides that directory for source
// checkouts, mirroring `SPECGATE_DISCOVERY_PATH`.
const WEAVER_PATH_VAR: &str = "SPECGATE_WEAVER_PATH";
const TARGETS_FILE: &str = "SpecGateCapture.targets";
// Discovery builds Debug, and an artifacts layout lowercases the configuration
// directory name, so capture builds the same configuration.
const CONFIGURATION: &str = "Debug";
const PROJECT_EXTENSION: &str = "csproj";
// `dotnet test --list-tests` prints this line, then one test per line indented
// by exactly four spaces.
const LIST_HEADER: &str = "The following Tests are available:";
const LIST_INDENT: &str = "    ";
// Every test result in a Visual Studio test-result log opens with this element.
const RESULT_ELEMENT: &str = "<UnitTestResult ";
const RUN_END: &str = "</TestRun>";
const OUTCOME_ATTRIBUTE: &str = "outcome=\"";
const PASSED: &str = "Passed";
const FAILED: &str = "Failed";
// Keep the SDK's output identical across machines so diagnostics and parsing do
// not depend on the user's locale, telemetry, or first-run banner.
const TOOL_ENVIRONMENT: [(&str, &str); 3] = [
    ("DOTNET_NOLOGO", "1"),
    ("DOTNET_CLI_TELEMETRY_OPTOUT", "1"),
    ("DOTNET_CLI_UI_LANGUAGE", "en"),
];
// Lines of build output kept in a failed build's diagnostic.
const DIAGNOSTIC_LINES: usize = 40;

#[cfg(test)]
mod tests;

mod adapter;
use adapter::Adapter;

/// One tests project built and run by the batch.
#[derive(Debug)]
struct Project {
    stem: String,
    path: PathBuf,
    artifacts: PathBuf,
    report: PathBuf,
    adapter: Adapter,
}

/// One enumerated test and the project that runs it.
#[derive(Debug)]
struct Listed {
    scenario_name: String,
    display_name: String,
    project: usize,
}

/// Everything a `dotnet` invocation of the batch has in common.
#[derive(Debug)]
struct Batch<'a> {
    sources_project: PathBuf,
    targets: PathBuf,
    framework: Option<&'a str>,
    results: PathBuf,
}

/// What one C# batch needs from the binding and the capture request.
#[derive(Debug)]
struct Plan<'a> {
    resolved: &'a ResolvedImplementation,
    requests: &'a [BundleRequest],
}

/// Build, enumerate, and run every test of the C# tests projects of `resolved`.
///
/// The sources are compiled exactly once per tests project, by the capture
/// build that weaves them. The metadata of the bound component and of every
/// component nested inside its captures is reflected from that build's output,
/// so no second compilation is needed; the result keeps every component's
/// normalized schema for publication.
///
/// Returns the selected component's discovery result and the executed tests, or
/// the first build, enumeration, weaving, or capture-stage failure.
pub(super) fn capture(
    system: &CommandEnvironment,
    execution: &Execution,
    discovery: &Discovery,
    resolved: &ResolvedImplementation,
    requests: &[BundleRequest],
    scratch: impl AsRef<Path>,
) -> Result<(Discovered, ExecutedTests), FailureContext> {
    run_batch(system, execution, discovery, &Plan { resolved, requests }, scratch.as_ref())
}

/// Run one planned batch: build and enumerate each project once, reflect the
/// built assembly, then run each listed test in its own process.
fn run_batch(
    system: &CommandEnvironment,
    execution: &Execution,
    discovery: &Discovery,
    plan: &Plan<'_>,
    scratch: &Path,
) -> Result<(Discovered, ExecutedTests), FailureContext> {
    let implementation = &plan.resolved.implementation;
    let Tests::Projects(declared) = &implementation.tests else {
        return Err(FailureContext::domain("C# capture requires tests.projects"));
    };
    let LanguageOptions::CSharp { framework } = &implementation.options else {
        return Err(FailureContext::domain("C# capture requires a C# implementation"));
    };
    let batch = Batch {
        sources_project: sources_project(system, &implementation.sources)?,
        targets: weaver_targets(system)?,
        framework: framework.as_deref(),
        results: scratch.join("results"),
    };
    let projects = resolve_projects(system, execution, declared, batch.framework, scratch)?;

    let mut listed = Vec::new();
    let mut woven = Vec::with_capacity(projects.len());
    for (index, project) in projects.iter().enumerate() {
        woven.push(build(system, execution, &batch, project)?);
        let names = list_tests(execution, &batch, project)?;
        listed.extend(names.into_iter().map(|display_name| Listed {
            scenario_name: if projects.len() > 1 {
                format!("{}::{display_name}", project.stem)
            } else {
                display_name.clone()
            },
            display_name,
            project: index,
        }));
    }
    listed.sort_by(|left, right| left.scenario_name.cmp(&right.scenario_name));
    if let Some(pair) = listed.windows(2).find(|pair| pair[0].scenario_name == pair[1].scenario_name) {
        return Err(FailureContext::domain(format!(
            "test display name '{}' is listed more than once; scenario names must identify exactly one test",
            pair[0].scenario_name
        )));
    }

    // Every project's build wove the same sources, so any one of them holds the
    // assembly to reflect.
    let reflected = discovery
        .discover_built(plan.resolved.clone(), &projects[0].artifacts)
        .map_err(|error| FailureContext::with_source(error.to_string(), error))?;
    let schemas = component_schemas(&reflected)?;
    let operations_discovered = !reflected.present_components.is_empty();
    let discovered = reflected
        .into_selected()
        .map_err(|error| FailureContext::with_source(error.to_string(), error))?;
    validate_setups(&discovered.registry, plan.requests)?;
    for (project, woven) in projects.iter().zip(woven) {
        require_woven(&batch, project, woven, operations_discovered)?;
    }

    system.create_dir_all(&batch.results).map_err(|error| {
        FailureContext::with_source(
            format!("failed to create test result directory {}: {error}", batch.results.display()),
            error,
        )
    })?;
    let sidecars = Sidecars::prepare(system, scratch)?;
    let mut executed = ExecutedTests {
        schemas,
        ..ExecutedTests::default()
    };
    for (index, test) in listed.iter().enumerate() {
        let issued = sidecars.issue(system, index, &test.scenario_name)?;
        let log = format!("{index:0INDEX_WIDTH$}.trx");
        let request = tool_request(
            "test",
            &batch,
            &projects[test.project],
            [
                OsString::from("--no-build"),
                OsString::from("--filter"),
                OsString::from(exact_filter(projects[test.project].adapter, &test.display_name)),
                OsString::from("--logger"),
                OsString::from(format!("trx;LogFileName={log}")),
                OsString::from("--results-directory"),
                batch.results.clone().into_os_string(),
            ],
            Some(&issued),
        );
        let output = execution
            .run(&request)
            .map_err(|error| FailureContext::with_source(format!("failed to run test '{}': {error}", test.scenario_name), error))?;
        let outcome = verify_selection(system, &batch.results.join(&log), &test.scenario_name)?;
        let settled = issued.settle(system, &test.scenario_name, &output)?;
        executed.record(apply_outcome(settled, &outcome, &test.scenario_name));
    }
    Ok((discovered, executed))
}

/// Keep the normalized schema of every component the built assembly declares.
///
/// A filtered capture keeps nested calls into other annotated components, and
/// the published registry must declare each of them, so every component's
/// outcome is retained until publication; one that failed to normalize stays an
/// error and only fails a bundle that actually contains it.
fn component_schemas(reflected: &specgate_discovery::output::Batch) -> Result<ComponentSchemas, FailureContext> {
    let mut schemas = ComponentSchemas::new();
    for component in &reflected.present_components {
        let schema = match reflected.schema(component.as_str()) {
            SchemaLookup::Found(schema) => serde_json::to_string(schema)
                .map_err(|error| FailureContext::with_source(format!("failed to serialize normalized discovery schema: {error}"), error))?,
            SchemaLookup::Invalid(error) => {
                schemas.insert(component.as_str().to_owned(), Err(error.to_string()));
                continue;
            }
            SchemaLookup::Missing => {
                return Err(FailureContext::domain(format!(
                    "component '{component}' is declared by the built assembly but discovery reported no metadata for it"
                )));
            }
        };
        schemas.insert(component.as_str().to_owned(), Ok(schema));
    }
    Ok(schemas)
}

/// Locate the weaver's targets file.
///
/// `SPECGATE_WEAVER_PATH` wins when set; otherwise the weaver is expected beside
/// the running `specgate` executable, which is how a published install is laid out.
fn weaver_targets(system: &CommandEnvironment) -> Result<PathBuf, FailureContext> {
    let directory = if let Some(value) = system.environment(WEAVER_PATH_VAR) {
        PathBuf::from(value)
    } else {
        system
            .executable_directory()
            .map_err(|error| FailureContext::with_source(format!("failed to locate the running specgate executable: {error}"), error))?
    };
    let targets = directory.join(TARGETS_FILE);
    if !system.is_file(&targets) {
        return Err(FailureContext::domain(format!(
            "C# capture requires {TARGETS_FILE} beside the specgate executable at {}; set {WEAVER_PATH_VAR} to its location \
             (from a source checkout, `just weaver-tool` builds it and `just test` sets the variable)",
            directory.display()
        )));
    }
    system
        .canonical(&targets)
        .map_err(|error| FailureContext::with_source(format!("failed to resolve {}: {error}", targets.display()), error))
}

/// Find the project that holds the instrumented sources: the first project file
/// directly inside the sources directory, the rule discovery uses too.
fn sources_project(system: &CommandEnvironment, sources: &Path) -> Result<PathBuf, FailureContext> {
    let project = system
        .files_with_extension(sources, PROJECT_EXTENSION)
        .map_err(|error| FailureContext::with_source(format!("failed to list {}: {error}", sources.display()), error))?
        .into_iter()
        .next()
        .ok_or_else(|| FailureContext::domain(format!("no .{PROJECT_EXTENSION} found under {}", sources.display())))?;
    system
        .canonical(&project)
        .map_err(|error| FailureContext::with_source(format!("failed to resolve {}: {error}", project.display()), error))
}

/// Resolve each declared tests project and reject a multi-targeted one when the
/// binding does not name a framework.
///
/// The framework list is read from the project's *evaluated* properties, so a
/// list inherited from `Directory.Build.props` or an imported file counts too.
fn resolve_projects(
    system: &CommandEnvironment,
    execution: &Execution,
    declared: &[PathBuf],
    framework: Option<&str>,
    scratch: &Path,
) -> Result<Vec<Project>, FailureContext> {
    let mut projects = Vec::with_capacity(declared.len());
    for (index, path) in declared.iter().enumerate() {
        let path = system
            .canonical(path)
            .map_err(|error| FailureContext::with_source(format!("tests project {} is not readable: {error}", path.display()), error))?;
        let adapter = inspect_project(execution, &path, framework)?;
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| FailureContext::domain(format!("could not derive a project name from {}", path.display())))?
            .to_string();
        let root = scratch.join(format!("p{index}"));
        system
            .create_dir_all(&root)
            .map_err(|error| FailureContext::with_source(format!("failed to create build directory {}: {error}", root.display()), error))?;
        projects.push(Project {
            stem,
            path,
            artifacts: root.join("artifacts"),
            report: root.join("weave.json"),
            adapter,
        });
    }
    Ok(projects)
}

/// Ask `MSBuild` what a tests project targets and which test framework it uses.
///
/// Both come from the project's *evaluated* state, so values inherited from
/// `Directory.Build.props`, an imported file, or central package management
/// count. When the binding chose a framework the project is evaluated under it,
/// because package references may be conditioned on the framework; otherwise
/// the project must target at most one, since several target frameworks
/// produce one test process each and duplicate display names.
fn inspect_project(execution: &Execution, project: &Path, framework: Option<&str>) -> Result<Adapter, FailureContext> {
    let mut builder = ProcessRequest::builder("dotnet").arg("msbuild").arg(project).args([
        "-getProperty:TargetFramework",
        "-getProperty:TargetFrameworks",
        "-getItem:PackageReference",
    ]);
    if let Some(framework) = framework {
        builder = builder.arg(format!("-p:TargetFramework={framework}"));
    }
    let request = pinned(builder).build();
    let output = execution
        .run(&request)
        .map_err(|error| FailureContext::with_source(format!("failed to query {}: {error}", project.display()), error))?;
    if !output.status.success() {
        return Err(FailureContext::domain(format!(
            "querying the target frameworks and test framework of {} failed:\n{}",
            project.display(),
            diagnostic(&output.stdout, &output.stderr)
        )));
    }
    let report = serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .ok()
        .filter(|report| report["Properties"].is_object())
        .ok_or_else(|| {
            FailureContext::domain(format!(
                "the target frameworks of {} could not be read from the MSBuild property report",
                project.display()
            ))
        })?;
    let frameworks = report["Properties"]["TargetFrameworks"]
        .as_str()
        .unwrap_or_default()
        .split(';')
        .filter(|framework| !framework.trim().is_empty())
        .count();
    if framework.is_none() && frameworks > 1 {
        return Err(FailureContext::domain(format!(
            "tests project {} targets several frameworks; set csharp.framework in the binding to choose one",
            project.display()
        )));
    }
    let packages = report["Items"]["PackageReference"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["Identity"].as_str());
    Adapter::detect(packages, project)
}

/// Build one tests project once, weaving the sources project as part of it, and
/// return how many methods the weaver instrumented.
///
/// After a successful build the weaver's report must exist; without it a batch
/// whose weaver never ran would look like tests that simply never reached an
/// instrumented operation.
fn build(system: &CommandEnvironment, execution: &Execution, batch: &Batch<'_>, project: &Project) -> Result<u64, FailureContext> {
    let request = tool_request(
        "build",
        batch,
        project,
        [
            OsString::from(format!("-p:SpecGateWeaveProject={}", batch.sources_project.display())),
            OsString::from(format!("-p:CustomAfterMicrosoftCommonTargets={}", batch.targets.display())),
            OsString::from(format!("-p:SpecGateWeaveReportPath={}", project.report.display())),
            OsString::from("-nodeReuse:false"),
            OsString::from("-p:UseSharedCompilation=false"),
        ],
        None,
    );
    let output = execution
        .run(&request)
        .map_err(|error| FailureContext::with_source(format!("failed to build {}: {error}", project.path.display()), error))?;
    if !output.status.success() {
        return Err(FailureContext::domain(format!(
            "C# capture build of {} failed:\n{}",
            project.path.display(),
            diagnostic(&output.stdout, &output.stderr)
        )));
    }
    let report = system.read(&project.report).map_err(|error| {
        FailureContext::with_source(
            format!(
                "the capture weaver did not report on the build of {}: {error}; the sources project {} was not woven",
                project.path.display(),
                batch.sources_project.display()
            ),
            error,
        )
    })?;
    serde_json::from_slice::<serde_json::Value>(&report)
        .ok()
        .and_then(|report| report["woven_method_count"].as_u64())
        .ok_or_else(|| FailureContext::domain(format!("the capture weaver report for {} is malformed", project.path.display())))
}

/// A batch whose discovery found operations but whose weaver wove no method
/// would look like tests that simply never reached an instrumented operation.
fn require_woven(batch: &Batch<'_>, project: &Project, woven: u64, operations_discovered: bool) -> Result<(), FailureContext> {
    if woven == 0 && operations_discovered {
        return Err(FailureContext::domain(format!(
            "the capture weaver wove no method of {} in the build of {} although discovery found operations there; tests would capture nothing",
            batch.sources_project.display(),
            project.path.display()
        )));
    }
    Ok(())
}

/// Enumerate the tests of one built project by display name.
fn list_tests(execution: &Execution, batch: &Batch<'_>, project: &Project) -> Result<Vec<String>, FailureContext> {
    let request = tool_request(
        "test",
        batch,
        project,
        [OsString::from("--no-build"), OsString::from("--list-tests")],
        None,
    );
    let output = execution
        .run(&request)
        .map_err(|error| FailureContext::with_source(format!("failed to list tests of {}: {error}", project.path.display()), error))?;
    if !output.status.success() {
        return Err(FailureContext::domain(format!(
            "C# capture test enumeration of {} failed:\n{}",
            project.path.display(),
            diagnostic(&output.stdout, &output.stderr)
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines().skip_while(|line| *line != LIST_HEADER);
    if lines.next().is_none() {
        return Err(FailureContext::domain(format!(
            "C# capture test enumeration of {} printed no test list",
            project.path.display()
        )));
    }
    // Everything after the header is scanned: warnings and blank lines between
    // entries are noise, and only indented lines name tests.
    let names: Vec<String> = lines
        .filter_map(|line| line.strip_prefix(LIST_INDENT))
        .map(str::to_string)
        .collect();
    if names.is_empty() {
        return Err(FailureContext::domain(format!(
            "C# capture test enumeration of {} listed no tests; a capture project must contain tests",
            project.path.display()
        )));
    }
    Ok(names)
}

/// Prove that one exact-name filter selected exactly one test, and return its
/// recorded outcome (`Passed`, `Failed`, `NotExecuted`, ...).
///
/// A filter that matches nothing exits successfully and logs no result, and a
/// name that is not unique selects several tests; either would silently
/// misattribute the sidecar, so both stop the batch. A missing or truncated
/// log is a crashed test host or a bad filter and stops the batch too, even
/// when the process succeeded: only a complete log that records a failed
/// result is an ordinary failed test.
fn verify_selection(system: &CommandEnvironment, log: &Path, scenario_name: &str) -> Result<String, FailureContext> {
    match system.read(log) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            if !text.contains(RUN_END) {
                return Err(FailureContext::domain(format!(
                    "the test result for '{scenario_name}' is incomplete; the test host likely crashed before finishing"
                )));
            }
            let mut results = text.match_indices(RESULT_ELEMENT);
            let selected = results.clone().count();
            let Some((start, _)) = results.next().filter(|_| selected == 1) else {
                return Err(FailureContext::domain(format!(
                    "the filter for '{scenario_name}' selected {selected} tests; a scenario name must match exactly one test"
                )));
            };
            text[start..]
                .split_once('>')
                .and_then(|(element, _)| element.split_once(OUTCOME_ATTRIBUTE))
                .and_then(|(_, rest)| rest.split_once('"'))
                .map(|(outcome, _)| outcome.to_string())
                .ok_or_else(|| FailureContext::domain(format!("the test result for '{scenario_name}' records no outcome")))
        }
        Err(error) => Err(FailureContext::with_source(
            format!("no result was logged for '{scenario_name}': the filter selected no test or the test host crashed ({error})"),
            error,
        )),
    }
}

/// Let the runner's recorded outcome veto a capture.
///
/// A finalized sidecar only proves the operations finished, not that the test
/// passed: a test can complete an operation and then skip or abort. Only a
/// `Passed` result keeps its capture; a `Failed` one is an ordinary failed test
/// and any other outcome is [`FailureReason::NotPassed`]. A test that recorded
/// nothing, or already failed, is left as settled.
fn apply_outcome(settled: Settled, outcome: &str, scenario_name: &str) -> Settled {
    if outcome == PASSED || !matches!(settled, Settled::Captured(_)) {
        return settled;
    }
    let (reason, summary) = if outcome == FAILED {
        (FailureReason::TestFailed, "the test result records a failed outcome".to_string())
    } else {
        (
            FailureReason::NotPassed,
            format!("the test finished with outcome {outcome} instead of Passed, so its recording was not published"),
        )
    };
    Settled::Failed(FailedTest {
        scenario_name: scenario_name.to_string(),
        reason,
        summary,
    })
}

/// Build the exact-match test filter for one listed name.
///
/// The filter grammar reserves `\ ( ) & | = ! ~` behind a backslash; that
/// grammar belongs to `VSTest`, not to a test framework, so it is the same for
/// every adapter. The value then passes through `MSBuild`, which decodes `%XX`
/// escapes and splits property lists on `,` and `;`, so those separators, the
/// quote that confuses the command-line hand-off, and the escape character `%`
/// itself are written as percent escapes (`%` first, so a literal `%2C` stays
/// literal). Only the property on the left differs per adapter.
fn exact_filter(adapter: Adapter, listed_name: &str) -> String {
    let mut filter = format!("{}=", adapter.filter_property());
    for character in listed_name.chars() {
        match character {
            '\\' | '(' | ')' | '&' | '|' | '=' | '!' | '~' => {
                filter.push('\\');
                filter.push(character);
            }
            '%' => filter.push_str("%25"),
            '"' => filter.push_str("%22"),
            ',' => filter.push_str("%2C"),
            ';' => filter.push_str("%3B"),
            other => filter.push(other),
        }
    }
    filter
}

/// Compose one `dotnet <verb> <project>` invocation of the batch, optionally
/// running under the capture configuration issued to one test.
fn tool_request(
    verb: &str,
    batch: &Batch<'_>,
    project: &Project,
    extra: impl IntoIterator<Item = OsString>,
    issued: Option<&Issued>,
) -> ProcessRequest {
    let mut builder = ProcessRequest::builder("dotnet")
        .arg(verb)
        .arg(&project.path)
        .args(["-c", CONFIGURATION, "--artifacts-path"])
        .arg(&project.artifacts);
    if let Some(framework) = batch.framework {
        builder = builder.args(["--framework", framework]);
    }
    builder = builder.args(extra);
    builder = pinned(builder);
    if let Some(issued) = issued {
        builder = builder.env(Issued::variable(), issued.environment());
    }
    builder.build()
}

/// Pin the SDK's output and keep any inherited capture configuration out of
/// the child: only the one test that was issued a configuration may see one.
fn pinned(mut builder: ProcessRequestBuilder) -> ProcessRequestBuilder {
    for (name, value) in TOOL_ENVIRONMENT {
        builder = builder.env(name, value);
    }
    builder.env_remove(Issued::variable())
}

/// Keep the first lines of a failed invocation's output.
fn diagnostic(stdout: &[u8], stderr: &[u8]) -> String {
    format!("{}\n{}", String::from_utf8_lossy(stderr), String::from_utf8_lossy(stdout))
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(DIAGNOSTIC_LINES)
        .collect::<Vec<_>>()
        .join("\n")
}
