//! Language-neutral results of a capture execution pass.
//!
//! This module allocates the scratch root, defines what one pass of isolated
//! test runs produced, and renders the bounded diagnostics for process
//! failures, persistence markers, and recordings that were never finalized. The
//! language runners that produce those results live beside it.
use super::{
    BTreeMap, Capture, CommandEnvironment, Completion, Digest, FAILURE_MARKER, FailureContext, Path, PathBuf, Sha256, TAIL_CHARS,
    TAIL_LINES,
};

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

/// Outcome of one capture execution pass over every enumerated test.
#[derive(Debug, Default)]
pub(super) struct ExecutedTests {
    pub(super) captures: Vec<Capture>,
    pub(super) failures: Vec<FailedTest>,
    /// Normalized schemas of every component the capture build's metadata
    /// declared, as serialized JSON, or why one could not be normalized. Empty
    /// for a language whose discovery registry already describes every
    /// component.
    pub(super) schemas: ComponentSchemas,
}

/// Normalized schemas, as serialized JSON, keyed by component id.
pub(super) type ComponentSchemas = BTreeMap<String, Result<String, String>>;

/// Why an enumerated test contributed no scenario to the batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FailureReason {
    /// The isolated test process exited unsuccessfully.
    TestFailed,
    /// The test reported success, but its recording was never finalized and so
    /// cannot be encoded. See
    /// `docs/decisions/top-level-abandonment-unsupported.md`.
    Unfinalized,
    /// The runner finished the test without a passing result (skipped, not
    /// executed, inconclusive, aborted), so whatever it recorded must not be
    /// published as a scenario.
    NotPassed,
    /// The test passed, but an operation observed a value CTSC cannot record
    /// against its declaration (a null in a non-nullable C# position). The
    /// scenario is dropped and the user is warned. See
    /// `docs/decisions/null-values-are-capture-diagnostics.md`.
    Unrecordable,
}

/// One enumerated test that produced no usable capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FailedTest {
    pub(super) scenario_name: String,
    pub(super) reason: FailureReason,
    /// For [`FailureReason::TestFailed`], exit status plus the tail of the
    /// test's own output; for [`FailureReason::Unfinalized`], why the recording
    /// could not be finalized. Deliberately not an artifact: it is
    /// machine-specific, and only ever reaches an error string.
    pub(super) summary: String,
}

/// Reject a capture batch in which any enumerated test produced no capture.
///
/// Only strict callers use this. Ordinary `specgate capture` skips such tests
/// and fails solely when a requested component is left with no scenario at all.
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
        "{} fixture test(s) produced no capture; every enumerated test must pass and leave a finalized recording:\n  {detail}",
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

/// Stable fault category a capture snapshot writes for an operation that was
/// still outstanding when the recording ended.
const INCOMPLETE_FAULT: &str = "incomplete_capture";

/// Report the operation that was still open when a recording ended, if any.
///
/// A failing test's sidecar is deleted before it is ever decoded, so an
/// `incomplete_capture` target fault reaching a decoded capture always comes
/// from a test that reported success, and encoding it would report an error for
/// a scenario that passed. Detection keys on that observable fact rather than on
/// a cause: more than one shape produces it — a top-level async abandonment, or
/// a target that exits the process inside an open operation — and every one of
/// them leaves a recording that could not be finalized, so all are treated
/// alike. See `docs/decisions/top-level-abandonment-unsupported.md`.
pub(super) fn unfinalized_operation(capture: &Capture) -> Option<&str> {
    capture
        .operations
        .iter()
        .find(|operation| matches!(&operation.completion, Some(Completion::Fault { fault_type, .. }) if fault_type == INCOMPLETE_FAULT))
        .map(|operation| operation.operation_name.as_str())
}

/// Explain why an unfinalized recording contributes no scenario.
pub(super) fn unfinalized_summary(operation: &str) -> String {
    format!(
        "operation '{operation}' was still open when the capture ended, so the recording could not be finalized and contributes no \
         scenario. Likely causes: an annotated async operation abandoned (dropped before completion) with no enclosing and no \
         subsequent annotated operation, or a target that exits the process inside an open operation. If the operation was abandoned, \
         enclose it in an annotated operation that completes."
    )
}

/// Stable fault category a capture runtime writes for an operation that
/// observed a value it cannot record against the operation's declaration.
const UNRECORDABLE_FAULT: &str = "specgate.unrecordable_value";

/// Report the first operation whose observed values could not be recorded, if
/// any, with the runtime's description of the offending position.
pub(super) fn unrecordable_operation(capture: &Capture) -> Option<(&str, &str)> {
    capture.operations.iter().find_map(|operation| match &operation.completion {
        Some(Completion::Fault { fault_type, message, .. }) if fault_type == UNRECORDABLE_FAULT => {
            Some((operation.operation_name.as_str(), message.as_str()))
        }
        _ => None,
    })
}

/// Explain why a scenario with an unrecordable value contributes no capture,
/// and ask whether the declaration should admit absence.
pub(super) fn unrecordable_summary(operation: &str, message: &str) -> String {
    format!(
        "operation '{operation}' observed a value that cannot be recorded ({message}); the scenario contributes no capture. If \
         absence is valid there, declare the position nullable (`T?`) so the registry declares it optional<T>; otherwise fix the \
         code that passed null."
    )
}

/// Render one warning line per scenario dropped for an unrecordable value.
pub(super) fn unrecordable_warnings(executed: &ExecutedTests) -> Vec<String> {
    executed
        .failures
        .iter()
        .filter(|failure| failure.reason == FailureReason::Unrecordable)
        .map(|failure| format!("{}: {}", failure.scenario_name, failure.summary))
        .collect()
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
