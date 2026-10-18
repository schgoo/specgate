//! C# capture runner: build the native test project with the capture weaver,
//! enumerate its tests with `dotnet test --list-tests`, and run each test in
//! its own `dotnet test` process.
//!
//! The decisions behind this runner are recorded in
//! `docs/decisions/csharp-capture-cli.md`. In short: the sources project is
//! woven during the build of each tests project through an `MSBuild` targets file
//! injected from the command line, so no project file is edited; every
//! enumerated test display name is one scenario; and a test-result log proves
//! that each exact-name filter selected exactly one test.
use super::sidecar::{Issued, Sidecars};
use super::{CommandEnvironment, Discovered, ExecutedTests, Execution, FailureContext, INDEX_WIDTH, Path, PathBuf, ProcessRequest};
use specgate_discovery::binding::{LanguageOptions, Tests};
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
// A project that builds several target frameworks produces one test process
// per framework and duplicate display names, so the binding must pick one.
const MULTI_TARGET_TAG: &str = "<TargetFrameworks>";
// `dotnet test --list-tests` prints this line, then one test per line indented
// by exactly four spaces.
const LIST_HEADER: &str = "The following Tests are available:";
const LIST_INDENT: &str = "    ";
// Every test result in a Visual Studio test-result log opens with this element.
const RESULT_ELEMENT: &str = "<UnitTestResult ";
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

/// One tests project built and run by the batch.
#[derive(Debug)]
struct Project {
    stem: String,
    path: PathBuf,
    artifacts: PathBuf,
    report: PathBuf,
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

/// Build, enumerate, and run every test of the C# tests projects of `discovered`.
///
/// Returns the first build, enumeration, weaving, or capture-stage failure.
pub(super) fn execute(
    system: &CommandEnvironment,
    execution: &Execution,
    discovered: &Discovered,
    scratch: impl AsRef<Path>,
) -> Result<ExecutedTests, FailureContext> {
    let scratch = scratch.as_ref();
    let implementation = &discovered.implementation.implementation;
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
    let projects = resolve_projects(system, declared, batch.framework, scratch)?;
    let operations_discovered = !discovered.registry.present_components().is_empty();

    let mut listed = Vec::new();
    for (index, project) in projects.iter().enumerate() {
        build(system, execution, &batch, project, operations_discovered)?;
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

    system.create_dir_all(&batch.results).map_err(|error| {
        FailureContext::with_source(
            format!("failed to create test result directory {}: {error}", batch.results.display()),
            error,
        )
    })?;
    let sidecars = Sidecars::prepare(system, scratch)?;
    let mut executed = ExecutedTests::default();
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
                OsString::from(exact_filter(&test.display_name)),
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
        verify_selection(system, &batch.results.join(&log), &test.scenario_name, output.status.success())?;
        executed.record(issued.settle(system, &test.scenario_name, &output)?);
    }
    Ok(executed)
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
fn resolve_projects(
    system: &CommandEnvironment,
    declared: &[PathBuf],
    framework: Option<&str>,
    scratch: &Path,
) -> Result<Vec<Project>, FailureContext> {
    let mut projects = Vec::with_capacity(declared.len());
    for (index, path) in declared.iter().enumerate() {
        let path = system
            .canonical(path)
            .map_err(|error| FailureContext::with_source(format!("tests project {} is not readable: {error}", path.display()), error))?;
        let text = system
            .read(&path)
            .map_err(|error| FailureContext::with_source(format!("failed to read {}: {error}", path.display()), error))?;
        if framework.is_none() && String::from_utf8_lossy(&text).contains(MULTI_TARGET_TAG) {
            return Err(FailureContext::domain(format!(
                "tests project {} targets several frameworks; set csharp.framework in the binding to choose one",
                path.display()
            )));
        }
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
        });
    }
    Ok(projects)
}

/// Build one tests project once, weaving the sources project as part of it.
///
/// After a successful build the weaver's report must exist; a batch whose
/// discovery found operations but whose weaver wove no method would otherwise
/// look like tests that simply never reached an instrumented operation.
fn build(
    system: &CommandEnvironment,
    execution: &Execution,
    batch: &Batch<'_>,
    project: &Project,
    operations_discovered: bool,
) -> Result<(), FailureContext> {
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
    let woven = serde_json::from_slice::<serde_json::Value>(&report)
        .ok()
        .and_then(|report| report["woven_method_count"].as_u64())
        .ok_or_else(|| FailureContext::domain(format!("the capture weaver report for {} is malformed", project.path.display())))?;
    if woven == 0 && operations_discovered {
        return Err(FailureContext::domain(format!(
            "the capture weaver wove no method of {} although discovery found operations there; tests would capture nothing",
            batch.sources_project.display()
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
    Ok(lines.map_while(|line| line.strip_prefix(LIST_INDENT)).map(str::to_string).collect())
}

/// Prove that one exact-name filter selected exactly one test.
///
/// A filter that matches nothing exits successfully and logs no result, and a
/// name that is not unique selects several tests; either would silently
/// misattribute the sidecar, so both stop the batch. A missing log after a
/// failed process is a crashed test host and is left to the caller as a failed
/// test.
fn verify_selection(system: &CommandEnvironment, log: &Path, scenario_name: &str, succeeded: bool) -> Result<(), FailureContext> {
    match system.read(log) {
        Ok(bytes) => {
            let selected = String::from_utf8_lossy(&bytes).matches(RESULT_ELEMENT).count();
            if selected == 1 {
                Ok(())
            } else {
                Err(FailureContext::domain(format!(
                    "the filter for '{scenario_name}' selected {selected} tests; a scenario name must match exactly one test"
                )))
            }
        }
        Err(_) if !succeeded => Ok(()),
        Err(error) => Err(FailureContext::with_source(
            format!("the filter for '{scenario_name}' selected no test: no result was logged ({error})"),
            error,
        )),
    }
}

/// Build the exact-match test filter for one display name.
///
/// The filter grammar reserves `\ ( ) & | = ! ~` behind a backslash, and the
/// separators `,` and `;` are written as percent escapes.
fn exact_filter(display_name: &str) -> String {
    let mut filter = String::from("DisplayName=");
    for character in display_name.chars() {
        match character {
            '\\' | '(' | ')' | '&' | '|' | '=' | '!' | '~' => {
                filter.push('\\');
                filter.push(character);
            }
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
    for (name, value) in TOOL_ENVIRONMENT {
        builder = builder.env(name, value);
    }
    if let Some(issued) = issued {
        builder = builder.env(Issued::variable(), issued.environment());
    }
    builder.build()
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
