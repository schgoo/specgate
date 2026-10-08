//! Isolated test execution and native-capture sidecar handling.
//!
//! This module allocates one scratch root, builds and enumerates eligible test
//! binaries, executes each test in a separate process, and consumes exactly one
//! atomic runtime sidecar per passing test. Process failures and persistence
//! markers become bounded diagnostics; missing sidecars mean the test did not
//! exercise the selected component rather than an execution failure.
use super::{
    BTreeSet, CAPTURE_ENV, CLOCK_STEP, Capture, CommandEnvironment, Config, ConfigDeps, Digest, EnvConfig, Execution, FAILURE_MARKER,
    FailureContext, INDEX_WIDTH, IsolatedTest, Path, PathBuf, ProcessRequest, START_TIME, Sha256, TAIL_CHARS, TAIL_LINES, TestBinary,
    cargo_bin, run_span, scenario_span, trace_id,
};

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

/// Allocate the command-scoped scratch directory used by build and sidecar artifacts.
///
/// Returns a contextual I/O error when allocation fails.
pub(super) fn scratch_dir(system: &CommandEnvironment, package_root: impl AsRef<Path>) -> Result<crate::system::Scratch, FailureContext> {
    let package_root = package_root.as_ref();
    let package_name = package_root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("could not derive package name from {}", package_root.display()))?;
    system
        .scratch(None::<&Path>, format!("specgate-capture-{package_name}-"))
        .map_err(|error| FailureContext::with_source("failed to create capture scratch directory", error))
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
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
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

/// Outcome of one capture execution pass over every enumerated test.
#[derive(Debug, Default)]
pub(super) struct ExecutedTests {
    pub(super) captures: Vec<Capture>,
    pub(super) failures: Vec<FailedTest>,
}

/// One enumerated test that failed while running under capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FailedTest {
    pub(super) scenario_name: String,
    /// Exit status plus the tail of the test's own output. Deliberately not an
    /// artifact: it is machine-specific, and only ever reaches an error string.
    pub(super) summary: String,
}

/// Reject a capture batch in which any enumerated test failed.
/// Reject any isolated test process that exited unsuccessfully.
pub(super) fn reject_failures(executed: &ExecutedTests) -> Result<(), FailureContext> {
    if executed.failures.is_empty() {
        return Ok(());
    }
    let detail = executed
        .failures
        .iter()
        .map(|failure| format!("{}: {}", failure.scenario_name, failure.summary))
        .collect::<Vec<_>>()
        .join("\n  ");
    Err(FailureContext::domain(format!(
        "{} fixture test(s) failed under capture; every enumerated test must pass:\n  {detail}",
        executed.failures.len()
    )))
}

/// Summarize one failed test run: its exit status and the tail of each stream.
/// Render a bounded process failure summary from its status and output tails.
pub(super) fn failure_summary(exit_code: Option<i32>, stdout: impl AsRef<[u8]>, stderr: impl AsRef<[u8]>) -> String {
    let stdout = stdout.as_ref();
    let stderr = stderr.as_ref();
    let mut parts = vec![exit_code.map_or_else(|| "terminated without an exit code".to_string(), |code| format!("exit code {code}"))];
    for (label, bytes) in [("stdout", stdout), ("stderr", stderr)] {
        let text = String::from_utf8_lossy(bytes);
        let mut tail = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .rev()
            .take(TAIL_LINES)
            .collect::<Vec<_>>();
        tail.reverse();
        if tail.is_empty() {
            continue;
        }
        let mut joined = tail.join(" | ");
        if joined.chars().count() > TAIL_CHARS {
            joined = joined.chars().take(TAIL_CHARS).collect::<String>() + "...";
        }
        parts.push(format!("{label}: {joined}"));
    }
    parts.join("; ")
}

/// Detect the runtime persistence-failure marker in process output.
pub(super) fn has_marker(bytes: impl AsRef<[u8]>) -> bool {
    bytes
        .as_ref()
        .windows(FAILURE_MARKER.len())
        .any(|window| window == FAILURE_MARKER.as_bytes())
}

/// Convert runtime sidecar persistence markers into a capture-stage error.
pub(super) fn reject_persistence(
    scenario_name: impl AsRef<str>,
    exit_code: Option<i32>,
    stdout: impl AsRef<[u8]>,
    stderr: impl AsRef<[u8]>,
) -> Result<(), FailureContext> {
    let scenario_name = scenario_name.as_ref();
    let stdout = stdout.as_ref();
    let stderr = stderr.as_ref();
    if has_marker(stdout) || has_marker(stderr) {
        return Err(FailureContext::domain(format!(
            "native capture persistence failed in '{scenario_name}': {}",
            failure_summary(exit_code, stdout, stderr)
        )));
    }
    Ok(())
}

/// Run every enumerated test once under deterministic native capture and return
/// the sidecars that passing tests produced, in stable scenario order.
///
/// Tests are component-agnostic here: one execution pass serves every requested
/// component, and scenario selection happens afterwards.
/// Run each test in isolation and decode sidecars from successful relevant tests.
///
/// A passing test without a sidecar is intentionally omitted. A failing test or
/// malformed sidecar stops the batch with test identity in the diagnostic.
pub(super) fn run_tests(
    system: &CommandEnvironment,
    execution: &Execution,
    tests: impl AsRef<[IsolatedTest]>,
    scratch: impl AsRef<Path>,
) -> Result<ExecutedTests, FailureContext> {
    let tests = tests.as_ref();
    let sidecars = scratch.as_ref().join("sidecars");
    system.create_dir_all(&sidecars).map_err(|error| {
        FailureContext::with_source(
            format!("failed to create capture sidecar directory {}: {error}", sidecars.display()),
            error,
        )
    })?;
    let mut captures = Vec::with_capacity(tests.len());
    let mut failures = Vec::with_capacity(tests.len());
    for (index, test) in tests.iter().enumerate() {
        let sidecar = sidecars.join(format!("{index:0INDEX_WIDTH$}.json"));
        let _ = system.remove_file(&sidecar);
        let environment = EnvConfig {
            capture: Config::builder(ConfigDeps {
                scenario_name: test.scenario_name.clone(),
                trace_id: trace_id(),
                run_id: run_span(),
                scenario_id: scenario_span(),
            })
            .start_time(START_TIME)
            .clock_step(CLOCK_STEP)
            .build()
            .expect("hard-coded deterministic capture configuration must be valid"),
            sidecar_path: sidecar.clone(),
        };
        let environment_json = serde_json::to_string(&environment)
            .map_err(|error| FailureContext::with_source(format!("failed to serialize native capture environment: {error}"), error))?;
        // Exact filtering and one test thread isolate each scenario so no
        // unrelated operation can enter its deterministic sidecar.
        let request = ProcessRequest::builder(&test.executable)
            .arg(&test.test_name)
            .args(["--exact", "--test-threads=1"])
            .env(CAPTURE_ENV, environment_json)
            .build();
        let output = execution.run(&request).map_err(|error| {
            FailureContext::with_source(format!("failed to run isolated test '{}': {error}", test.scenario_name), error)
        })?;
        reject_persistence(&test.scenario_name, output.status.code(), &output.stdout, &output.stderr)?;
        if !output.status.success() {
            let _ = system.remove_file(&sidecar);
            failures.push(FailedTest {
                scenario_name: test.scenario_name.clone(),
                summary: failure_summary(output.status.code(), &output.stdout, &output.stderr),
            });
            continue;
        }
        if !system.is_file(&sidecar) {
            continue;
        }
        let bytes = system.read(&sidecar).map_err(|error| {
            FailureContext::with_source(
                format!("failed to read native capture sidecar {}: {error}", sidecar.display()),
                error,
            )
        })?;
        let _ = system.remove_file(&sidecar);
        let capture: Capture = serde_json::from_slice(&bytes).map_err(|error| {
            FailureContext::with_source(
                format!("failed to parse native capture sidecar for '{}': {error}", test.scenario_name),
                error,
            )
        })?;
        if capture.operations.is_empty() {
            continue;
        }
        captures.push(capture);
    }
    captures.shrink_to_fit();
    failures.shrink_to_fit();
    Ok(ExecutedTests { captures, failures })
}
/// Persist one final bundle artifact through the injected command environment.
pub(super) fn write_artifact(system: &CommandEnvironment, path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> Result<(), FailureContext> {
    let path = path.as_ref();
    system
        .write(path, bytes)
        .map_err(|error| FailureContext::with_source(format!("failed to write capture artifact {}: {error}", path.display()), error))
}

/// Return the lowercase SHA-256 digest used by capture manifest linkage.
pub(super) fn sha256_digest(bytes: impl AsRef<[u8]>) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes.as_ref()))
}

/// Resolve one fixed artifact filename below a caller-provided output root.
pub(super) fn artifact_path(out: impl AsRef<Path>, filename: impl AsRef<Path>) -> PathBuf {
    out.as_ref().join(filename)
}
