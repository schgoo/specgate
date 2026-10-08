//! Generated-code failure reporting and parent-process protocol output.

use super::*;
#[cfg(feature = "test-util")]
use std::sync::{Arc, Mutex};

/// Parent-process capture protocol marker consumed by the `SpecGate` CLI.
///
/// The exact spelling preserves generated-runner failure detection; changing it
/// requires a coordinated runtime, CLI, replay, and protocol compatibility update.
pub const FAILURE_MARKER: &str = "__SPECGATE_NATIVE_CAPTURE_PERSISTENCE_FAILURE__";

/// Stable event name consumed by capture telemetry subscribers.
const FAILURE_EVENT: &str = "specgate.capture.persistence_failure";
/// Stable error classification consumed by capture telemetry subscribers.
const PERSISTENCE_ERROR: &str = "capture.persistence";
/// Stable instrumentation stage attached to generated capture failures.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
#[expect(
    clippy::exhaustive_enums,
    reason = "generated macro/runtime protocol is versioned as one closed surface"
)]
pub enum Stage {
    /// Beginning an operation scope.
    OperationBegin,
    /// Recording an operation input.
    OperationInput,
    /// Completing an operation scope.
    OperationComplete,
    /// Committing setup provenance.
    SetupCommit,
    /// Recording a native observation.
    Observation,
    /// Reporting a generated completion failure.
    Completion,
}

impl Stage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::OperationBegin => "operation.begin",
            Self::OperationInput => "operation.input",
            Self::OperationComplete => "operation.complete",
            Self::SetupCommit => "setup.commit",
            Self::Observation => "observation.record",
            Self::Completion => "capture.completion",
        }
    }
}

fn report_at(stage: Stage, error: impl AsRef<str>) {
    let error = error.as_ref();
    let marked = if error.contains(FAILURE_MARKER) {
        error.to_string()
    } else {
        format!("{FAILURE_MARKER} {error}")
    };
    capture::mark_error(&CaptureError::message(CaptureErrorKind::Persistence, marked.clone()));
    report_telemetry(&FailureTelemetry {
        event_name: FAILURE_EVENT,
        error_type: PERSISTENCE_ERROR,
        stage: stage.as_str(),
        protocol_marker: &marked,
    });
}

struct FailureTelemetry<'a> {
    event_name: &'static str,
    error_type: &'static str,
    stage: &'static str,
    protocol_marker: &'a str,
}

#[derive(Debug, Clone)]
struct ProtocolOutput {
    backend: ProtocolBackend,
}

#[derive(Debug, Clone)]
enum ProtocolBackend {
    Real,
    #[cfg(feature = "test-util")]
    Fake(Arc<Mutex<Vec<String>>>),
}

impl ProtocolOutput {
    const fn real() -> Self {
        Self {
            backend: ProtocolBackend::Real,
        }
    }

    #[cfg(feature = "test-util")]
    fn fake(records: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            backend: ProtocolBackend::Fake(records),
        }
    }

    fn write(&self, marker: impl AsRef<str>) {
        let marker = marker.as_ref();
        match &self.backend {
            ProtocolBackend::Real => {
                let mut standard_error = std::io::stderr().lock();
                let _ignored = writeln!(standard_error, "{marker}");
            }
            #[cfg(feature = "test-util")]
            ProtocolBackend::Fake(records) => records
                .lock()
                .expect("generated protocol output mutex poisoned")
                .push(marker.to_string()),
        }
    }
}

thread_local! {
    static PROTOCOL_OUTPUT: RefCell<ProtocolOutput> = const { RefCell::new(ProtocolOutput::real()) };
}

#[cfg(feature = "test-util")]
/// Thread-local fake parent-protocol output controller for deterministic tests.
#[derive(Debug, Clone)]
pub struct Output {
    records: Arc<Mutex<Vec<String>>>,
}

#[cfg(feature = "test-util")]
impl Output {
    /// Install a thread-local fake parent-protocol sink.
    #[must_use]
    pub fn install() -> Self {
        capture_output()
    }
    /// Restore real parent-protocol stderr output.
    pub fn reset(self) {
        drop(self);
        reset_output();
    }

    /// Return captured parent-protocol records.
    ///
    /// # Panics
    /// Panics if another test panicked while mutating the shared record buffer.
    #[must_use]
    pub fn records(&self) -> Vec<String> {
        self.records.lock().expect("generated protocol output mutex poisoned").clone()
    }
}

#[cfg(feature = "test-util")]
pub(crate) fn capture_output() -> Output {
    let records = Arc::new(Mutex::new(Vec::new()));
    PROTOCOL_OUTPUT.with(|output| *output.borrow_mut() = ProtocolOutput::fake(Arc::clone(&records)));
    Output { records }
}

#[cfg(feature = "test-util")]
pub(crate) fn reset_output() {
    PROTOCOL_OUTPUT.with(|output| *output.borrow_mut() = ProtocolOutput::real());
}

fn report_telemetry(failure: &FailureTelemetry<'_>) {
    assert!(!failure.event_name.is_empty(), "generated failure event name must not be empty");
    assert!(!failure.error_type.is_empty(), "generated failure error type must not be empty");
    assert!(!failure.stage.is_empty(), "generated failure stage must not be empty");
    tracing::event!(
        name: FAILURE_EVENT,
        tracing::Level::ERROR,
        error.type = failure.error_type,
        capture.stage = failure.stage,
        protocol.marker = FAILURE_MARKER,
        "generated capture failure: {{error.type}} stage={{capture.stage}} protocol_marker={{protocol.marker}}"
    );
    PROTOCOL_OUTPUT.with(|output| output.borrow().write(failure.protocol_marker));
}

/// Validate and report a generated capture failure at a named instrumentation stage.
///
/// # Panics
/// Panics when a test has poisoned the feature-gated fake output mutex.
///
#[doc(hidden)]
pub fn report_error(stage: Stage, error: impl AsRef<str>) {
    let message = match error.as_ref() {
        "" => "generated capture failure omitted a diagnostic",
        message => message,
    };
    report_at(stage, message);
}

/// Report an explicit generated completion failure to the parent capture process.
///
/// `Ok(())` is a no-op. An `Err` is validated and emitted through the structured
/// failure channel and the stable parent-process protocol marker.
///
/// # Examples
/// ```
/// # #[cfg(feature = "test-util")] {
/// let output = specgate_runtime::generated::Output::install();
/// specgate_runtime::generated::report_completion::<&str>(Ok(()));
/// assert!(output.records().is_empty());
/// specgate_runtime::generated::report_completion(Err("sidecar unavailable"));
/// assert_eq!(output.records().len(), 1);
/// output.reset();
/// # }
/// ```
///
/// # Panics
/// Panics for a poisoned test-output mutex.
#[doc(hidden)]
pub fn report_completion<E: AsRef<str>>(result: Result<(), E>) {
    if let Err(error) = result {
        report_error(Stage::Completion, &error);
    }
}
