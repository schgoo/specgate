//! Rust capture runner: build libtest binaries with Cargo, enumerate their
//! tests, and run each one in its own process.
//!
//! One scratch root serves the whole batch. Each test runs once under
//! deterministic native capture, and its sidecar is settled by the shared
//! sidecar protocol; this module owns only what is specific to Cargo and libtest.
use super::sidecar::{Issued, Sidecars};
use super::{BTreeSet, CommandEnvironment, ExecutedTests, Execution, FailureContext, Path, PathBuf, ProcessRequest, cargo_bin};

// Cargo's `--message-format=json` schema identifies test binaries with these
// exact reason/profile/target-kind tokens. Changing them would silently omit
// eligible artifacts from capture.
const ARTIFACT_REASON: &str = "compiler-artifact";
const TEST_PROFILE: &str = "test";
const LIBRARY_KIND: &str = "lib";
const FALLBACK_LABEL: &str = "test";
// Stable libtest `--list --format terse` suffix. A different suffix would
// prevent enumerated test cases from being isolated into capture processes.
const CASE_SUFFIX: &str = ": test";

/// One Cargo-built libtest executable.
#[derive(Debug)]
pub(super) struct TestBinary {
    pub(super) label: String,
    pub(super) executable: PathBuf,
    pub(super) is_library: bool,
}

/// One libtest case that runs in its own process.
#[derive(Debug)]
pub(super) struct IsolatedTest {
    pub(super) scenario_name: String,
    pub(super) test_name: String,
    pub(super) executable: PathBuf,
}

/// Build, enumerate, and run every test of the Rust package at `sources`.
///
/// Returns the first build, enumeration, or capture-stage failure.
pub(super) fn execute(
    system: &CommandEnvironment,
    execution: &Execution,
    sources: impl AsRef<Path>,
    scratch: impl AsRef<Path>,
) -> Result<ExecutedTests, FailureContext> {
    let scratch = scratch.as_ref();
    let test_binaries = build_binaries(execution, sources, scratch)?;
    let tests = enumerate_tests(execution, &test_binaries)?;
    run_tests(system, execution, &tests, scratch)
}

/// Build eligible test targets and return their executable metadata in stable order.
///
/// Returns a bounded compiler diagnostic when Cargo fails or emits malformed metadata.
pub(super) fn build_binaries(
    execution: &Execution,
    package_root: impl AsRef<Path>,
    scratch: impl AsRef<Path>,
) -> Result<Vec<TestBinary>, FailureContext> {
    let package_root = package_root.as_ref();
    let scratch = scratch.as_ref();
    let request = ProcessRequest::builder(cargo_bin())
        .args(["test", "--no-run", "--quiet", "--message-format=json"])
        .current_dir(package_root)
        .env_remove("CARGO")
        .env_remove("CARGO_MANIFEST_DIR")
        .env("CARGO_TARGET_DIR", scratch.join("target"))
        .build();
    let output = execution
        .run(&request)
        .map_err(|error| FailureContext::with_source(format!("failed to build capture test binaries: {error}"), error))?;
    if !output.status.success() {
        return Err(FailureContext::domain(format!(
            "capture test build failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let mut binaries = Vec::new();
    let mut seen = BTreeSet::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if message.get("reason").and_then(serde_json::Value::as_str) != Some(ARTIFACT_REASON)
            || !message
                .get("profile")
                .and_then(|profile| profile.get(TEST_PROFILE))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        {
            continue;
        }
        let Some(executable) = message.get("executable").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let executable = PathBuf::from(executable);
        if !seen.insert(executable.clone()) {
            continue;
        }
        let target = &message["target"];
        let label = target["name"].as_str().unwrap_or(FALLBACK_LABEL).to_string();
        let is_library = target["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some(LIBRARY_KIND)));
        binaries.push(TestBinary {
            label,
            executable,
            is_library,
        });
    }
    binaries.sort_by(|left, right| {
        right
            .is_library
            .cmp(&left.is_library)
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.executable.cmp(&right.executable))
    });
    if binaries.is_empty() {
        return Err(FailureContext::domain("capture test build produced no libtest binaries"));
    }
    binaries.shrink_to_fit();
    Ok(binaries)
}

/// Enumerate non-ignored tests from each built binary without executing test bodies.
///
/// Returns a process diagnostic when a binary cannot be listed.
pub(super) fn enumerate_tests(execution: &Execution, binaries: impl AsRef<[TestBinary]>) -> Result<Vec<IsolatedTest>, FailureContext> {
    let binaries = binaries.as_ref();
    let mut tests = Vec::new();
    for binary in binaries {
        let request = ProcessRequest::builder(&binary.executable)
            .args(["--list", "--format", "terse"])
            .build();
        let output = execution.run(&request).map_err(|error| {
            FailureContext::with_source(format!("failed to list tests in {}: {error}", binary.executable.display()), error)
        })?;
        if !output.status.success() {
            return Err(FailureContext::domain(format!(
                "capture test enumeration failed for {}: {}",
                binary.executable.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let Some(test_name) = line.strip_suffix(CASE_SUFFIX) else {
                continue;
            };
            let test_name = test_name.trim().to_string();
            let scenario_name = if binary.is_library {
                test_name.clone()
            } else {
                format!("{}::{test_name}", binary.label)
            };
            tests.push(IsolatedTest {
                scenario_name,
                test_name,
                executable: binary.executable.clone(),
            });
        }
    }
    tests.sort_by(|left, right| left.scenario_name.cmp(&right.scenario_name));
    tests.shrink_to_fit();
    Ok(tests)
}

/// Run every enumerated test once under deterministic native capture and return
/// the sidecars that passing tests produced, in stable scenario order.
///
/// Tests are component-agnostic here: one execution pass serves every requested
/// component, and scenario selection happens afterwards. A passing test without
/// a sidecar is intentionally omitted. A failing test, and a passing test whose
/// recording was never finalized, each contribute no scenario and are recorded
/// as failures for the caller to weigh. A malformed sidecar stops the batch with
/// test identity in the diagnostic.
pub(super) fn run_tests(
    system: &CommandEnvironment,
    execution: &Execution,
    tests: impl AsRef<[IsolatedTest]>,
    scratch: impl AsRef<Path>,
) -> Result<ExecutedTests, FailureContext> {
    let tests = tests.as_ref();
    let sidecars = Sidecars::prepare(system, scratch)?;
    let mut executed = ExecutedTests::default();
    executed.captures.reserve(tests.len());
    executed.failures.reserve(tests.len());
    for (index, test) in tests.iter().enumerate() {
        let issued = sidecars.issue(system, index, &test.scenario_name)?;
        // Exact filtering isolates the scenario, and one test thread is a
        // correctness requirement rather than only a determinism one: every
        // capture session in this process shares one resolved scenario identity
        // and one sidecar path, so concurrent capture threads would overwrite
        // one another. Per-session identity is tracked by
        // https://github.com/schgoo/specgate/issues/70.
        let request = ProcessRequest::builder(&test.executable)
            .arg(&test.test_name)
            .args(["--exact", "--test-threads=1"])
            .env(Issued::variable(), issued.environment())
            .build();
        let output = execution.run(&request).map_err(|error| {
            FailureContext::with_source(format!("failed to run isolated test '{}': {error}", test.scenario_name), error)
        })?;
        executed.record(issued.settle(system, &test.scenario_name, &output)?);
    }
    executed.captures.shrink_to_fit();
    executed.failures.shrink_to_fit();
    Ok(executed)
}
