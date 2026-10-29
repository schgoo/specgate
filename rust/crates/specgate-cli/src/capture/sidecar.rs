//! Per-test native-capture sidecar protocol shared by every language runner.
//!
//! A runner issues one deterministic environment configuration per isolated
//! test, runs the test with it, and settles the outcome here: the process
//! result and the sidecar the test left behind become one capture, a skip, or a
//! recorded failure. Language runners differ only in how they build, enumerate,
//! and launch a test.
use super::{
    CAPTURE_ENV, CLOCK_STEP, Capture, CommandEnvironment, Config, ConfigDeps, EnvConfig, ExecutedTests, FailedTest, FailureContext,
    FailureReason, INDEX_WIDTH, Path, PathBuf, START_TIME, failure_summary, reject_persistence, run_span, scenario_span, trace_id,
    unfinalized_operation, unfinalized_summary, unrecordable_operation, unrecordable_summary,
};
use std::process::Output;

/// The directory that holds one sidecar per isolated test of a batch.
#[derive(Debug)]
pub(super) struct Sidecars {
    directory: PathBuf,
}

/// The environment one isolated test is run with and the sidecar it writes.
#[derive(Debug)]
pub(super) struct Issued {
    path: PathBuf,
    environment: String,
}

/// What one isolated test contributed to the batch.
#[derive(Debug)]
pub(super) enum Settled {
    /// A finalized recording of at least one operation.
    Captured(Capture),
    /// No usable recording and nothing wrong: the test never reached an
    /// instrumented operation.
    Skipped,
    /// The test produced no scenario; the caller weighs the failure.
    Failed(FailedTest),
}

impl Sidecars {
    /// Create the sidecar directory below the batch scratch root.
    ///
    /// Returns a contextual I/O error when the directory cannot be created.
    pub(super) fn prepare(system: &CommandEnvironment, scratch: impl AsRef<Path>) -> Result<Self, FailureContext> {
        let directory = scratch.as_ref().join("sidecars");
        system.create_dir_all(&directory).map_err(|error| {
            FailureContext::with_source(
                format!("failed to create capture sidecar directory {}: {error}", directory.display()),
                error,
            )
        })?;
        Ok(Self { directory })
    }

    /// Issue the deterministic configuration of the test at `index`.
    ///
    /// Any sidecar left at the issued path is removed first, so a recording
    /// found afterwards was written by this test's run.
    ///
    /// Returns a serialization error when the configuration cannot be encoded.
    pub(super) fn issue(&self, system: &CommandEnvironment, index: usize, scenario_name: &str) -> Result<Issued, FailureContext> {
        let path = self.directory.join(format!("{index:0INDEX_WIDTH$}.json"));
        let _ = system.remove_file(&path);
        let environment = EnvConfig {
            capture: Config::builder(ConfigDeps {
                scenario_name: scenario_name.to_string(),
                trace_id: trace_id(),
                run_id: run_span(),
                scenario_id: scenario_span(),
            })
            .start_time(START_TIME)
            .clock_step(CLOCK_STEP)
            .build()
            .expect("hard-coded deterministic capture configuration must be valid"),
            sidecar_path: path.clone(),
        };
        let environment = serde_json::to_string(&environment)
            .map_err(|error| FailureContext::with_source(format!("failed to serialize native capture environment: {error}"), error))?;
        Ok(Issued { path, environment })
    }
}

impl Issued {
    /// The environment variable the test process reads its configuration from.
    pub(super) const fn variable() -> &'static str {
        CAPTURE_ENV
    }

    /// The serialized configuration the test process is started with.
    pub(super) fn environment(&self) -> &str {
        &self.environment
    }

    /// Turn one finished test process and its sidecar into a [`Settled`] outcome.
    ///
    /// A persistence failure marker in the process output stops the batch. A
    /// failed process and a recording that was never finalized each contribute
    /// no scenario; a passing process that left no sidecar, or one with no
    /// operations, is skipped. A sidecar that cannot be read or parsed stops
    /// the batch with the scenario's identity in the diagnostic.
    pub(super) fn settle(
        &self,
        system: &CommandEnvironment,
        scenario_name: impl AsRef<str>,
        output: &Output,
    ) -> Result<Settled, FailureContext> {
        let scenario_name = scenario_name.as_ref();
        reject_persistence(scenario_name, output.status.code(), &output.stdout, &output.stderr)?;
        if !output.status.success() {
            let _ = system.remove_file(&self.path);
            return Ok(Settled::Failed(FailedTest {
                scenario_name: scenario_name.to_string(),
                reason: FailureReason::TestFailed,
                summary: failure_summary(output.status.code(), &output.stdout, &output.stderr),
            }));
        }
        if !system.is_file(&self.path) {
            return Ok(Settled::Skipped);
        }
        let bytes = system.read(&self.path).map_err(|error| {
            FailureContext::with_source(
                format!("failed to read native capture sidecar {}: {error}", self.path.display()),
                error,
            )
        })?;
        let _ = system.remove_file(&self.path);
        let capture: Capture = serde_json::from_slice(&bytes).map_err(|error| {
            FailureContext::with_source(
                format!("failed to parse native capture sidecar for '{scenario_name}': {error}"),
                error,
            )
        })?;
        if capture.scenario_name != scenario_name
            || capture.trace_id != trace_id()
            || capture.run.span_id != run_span()
            || capture.scenario.span_id != scenario_span()
        {
            return Err(FailureContext::domain(format!(
                "native capture sidecar for '{scenario_name}' does not carry the identity issued to that test (it names scenario '{}'); \
                 the sidecar was not written by this run",
                capture.scenario_name
            )));
        }
        if capture.operations.is_empty() {
            return Ok(Settled::Skipped);
        }
        if let Some(operation) = unfinalized_operation(&capture) {
            return Ok(Settled::Failed(FailedTest {
                scenario_name: scenario_name.to_string(),
                reason: FailureReason::Unfinalized,
                summary: unfinalized_summary(operation),
            }));
        }
        if let Some((operation, message)) = unrecordable_operation(&capture) {
            return Ok(Settled::Failed(FailedTest {
                scenario_name: scenario_name.to_string(),
                reason: FailureReason::Unrecordable,
                summary: unrecordable_summary(operation, message),
            }));
        }
        Ok(Settled::Captured(capture))
    }
}

impl ExecutedTests {
    /// Fold one settled test into the batch.
    pub(super) fn record(&mut self, settled: Settled) {
        match settled {
            Settled::Captured(capture) => self.captures.push(capture),
            Settled::Failed(failure) => self.failures.push(failure),
            Settled::Skipped => {}
        }
    }
}
