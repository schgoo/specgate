//! Child-process requests and concrete real/fake process adapters.
//!
//! Callers construct a complete request before execution, keeping command-line
//! and environment mutation separate from side effects:
//!
//! ```text
//! let request = Request::builder("cargo").arg("metadata").current_dir(root).build();
//! let output = Process::real().output(&request)?;
//! ```
//!
//! Focused tests inject `Process::fake(FakeProcess::queued(...))`. Each call to
//! `output` records the request and consumes exactly one queued result, so tests
//! can verify invocation order and deterministically inject operational errors.
//! Exhausting that queue is a test-contract panic rather than a process error.

#[cfg(any(test, feature = "test-util"))]
use super::failure;
use crate::error::Error;
#[cfg(any(test, feature = "test-util"))]
use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(any(test, feature = "test-util"))]
use std::sync::{Arc, Mutex};
/// Complete, side-effect-free description of one child-process invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) executable: OsString,
    pub(crate) args: Vec<OsString>,
    pub(crate) current_dir: Option<PathBuf>,
    pub(crate) env_remove: Vec<OsString>,
    pub(crate) env: Vec<(OsString, OsString)>,
}

impl Request {
    /// Starts a request for the required executable.
    /// Returns the configured request.
    pub(crate) fn builder(executable: impl AsRef<OsStr>) -> RequestBuilder {
        RequestBuilder {
            request: Self {
                executable: executable.as_ref().to_os_string(),
                args: Vec::new(),
                current_dir: None,
                env_remove: Vec::new(),
                env: Vec::new(),
            },
        }
    }
}

/// Fluent configuration for a [`Request`].
pub(crate) struct RequestBuilder {
    request: Request,
}

impl RequestBuilder {
    /// Appends one command-line argument.
    pub(crate) fn arg(mut self, value: impl AsRef<OsStr>) -> Self {
        self.request.args.push(value.as_ref().to_os_string());
        self
    }
    /// Sets the child process working directory.
    pub(crate) fn current_dir(mut self, value: impl AsRef<Path>) -> Self {
        self.request.current_dir = Some(value.as_ref().to_path_buf());
        self
    }
    /// Removes one inherited environment variable.
    pub(crate) fn env_remove(mut self, value: impl AsRef<OsStr>) -> Self {
        self.request.env_remove.push(value.as_ref().to_os_string());
        self
    }
    /// Sets one child-process environment variable.
    pub(crate) fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.request.env.push((key.as_ref().to_os_string(), value.as_ref().to_os_string()));
        self
    }
    /// Returns the configured request.
    pub(crate) fn build(self) -> Request {
        self.request
    }
}

/// Captured status and byte streams from a completed child process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessOutput {
    pub(crate) success: bool,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

/// Executes requests through the operating system or a deterministic fake queue.
#[derive(Clone)]
pub(crate) struct Process {
    inner: ProcessKind,
}
#[derive(Clone)]
enum ProcessKind {
    Real,
    #[cfg(any(test, feature = "test-util"))]
    Fake(Arc<Mutex<FakeProcess>>),
}
#[cfg(any(test, feature = "test-util"))]
/// Queued outcomes and captured requests for deterministic process tests.
#[derive(Default)]
pub(crate) struct FakeProcess {
    results: VecDeque<Result<ProcessOutput, Error>>,
    pub(crate) requests: Vec<Request>,
}
#[cfg(any(test, feature = "test-util"))]
impl FakeProcess {
    /// Build fake process state from queued operational outcomes.
    pub(crate) fn queued(results: impl IntoIterator<Item = Result<ProcessOutput, String>>) -> Self {
        let mut results: VecDeque<_> = results.into_iter().map(|result| result.map_err(failure)).collect();
        results.shrink_to_fit();
        let request_capacity = results.len();
        Self {
            results,
            requests: Vec::with_capacity(request_capacity),
        }
    }
}

impl Process {
    /// Creates an operating-system-backed adapter.
    pub(super) fn real() -> Self {
        Self { inner: ProcessKind::Real }
    }
    #[cfg(any(test, feature = "test-util"))]
    /// Creates a deterministic adapter from queued fake state.
    pub(super) fn fake(state: FakeProcess) -> Self {
        Self {
            inner: ProcessKind::Fake(Arc::new(Mutex::new(state))),
        }
    }
    #[cfg(test)]
    /// Returns shared fake state for focused assertions.
    ///
    /// # Panics
    /// Panics when called for a real process adapter.
    pub(crate) fn fake_state(&self) -> Arc<Mutex<FakeProcess>> {
        match &self.inner {
            ProcessKind::Fake(state) => Arc::clone(state),
            ProcessKind::Real => panic!("real process has no fake state"),
        }
    }
    /// Executes or dequeues the requested process and captures its output.
    ///
    /// # Errors
    /// Returns process-spawn, wait, or injected fake failures.
    pub(crate) fn output(&self, request: &Request) -> Result<ProcessOutput, Error> {
        match &self.inner {
            ProcessKind::Real => {
                let mut command = Command::new(&request.executable);
                command.args(&request.args);
                if let Some(path) = &request.current_dir {
                    command.current_dir(path);
                }
                for name in &request.env_remove {
                    command.env_remove(name);
                }
                for (key, value) in &request.env {
                    command.env(key, value);
                }
                let output = command.output()?;
                Ok(ProcessOutput {
                    success: output.status.success(),
                    stdout: output.stdout,
                    stderr: output.stderr,
                })
            }
            #[cfg(any(test, feature = "test-util"))]
            ProcessKind::Fake(state) => {
                let mut state = state.lock().expect("fake process mutex poisoned");
                state.requests.push(request.clone());
                let output = state
                    .results
                    .pop_front()
                    .unwrap_or_else(|| panic!("fake process queue exhausted; enqueue an operational result"))?;
                Ok(output)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_chain() {
        let request = Request::builder("tool")
            .arg("run")
            .current_dir("work")
            .env_remove("OLD")
            .env("NEW", "value")
            .build();
        assert_eq!(request.args, [OsString::from("run")]);
        assert_eq!(request.current_dir.as_deref(), Some(Path::new("work")));
        assert_eq!(request.env_remove, [OsString::from("OLD")]);
        assert_eq!(request.env, [(OsString::from("NEW"), OsString::from("value"))]);
    }

    #[test]
    fn fake_execution() {
        let expected = ProcessOutput {
            success: true,
            stdout: b"ok".to_vec(),
            stderr: Vec::new(),
        };
        let process = Process::fake(FakeProcess::queued([Ok(expected.clone())]));
        let request = Request::builder("tool").arg("run").build();
        assert_eq!(process.output(&request).unwrap(), expected);
        assert_eq!(process.fake_state().lock().unwrap().requests, [request]);
    }
    #[test]
    fn queued_error() {
        let process = Process::fake(FakeProcess::queued([Err("injected".to_string())]));
        let error = process.output(&Request::builder("tool").build()).unwrap_err();
        assert!(error.to_string().contains("injected"));
    }

    #[test]
    #[should_panic(expected = "fake process queue exhausted")]
    fn queue_exhaustion() {
        let process = Process::fake(FakeProcess::default());
        let _ = process.output(&Request::builder("tool").build());
    }

    #[test]
    fn real_execution() {
        let executable = std::env::current_exe().expect("current test executable");
        let request = Request::builder(executable).arg("--list").env("SPECGATE_PROCESS_TEST", "1").build();
        let output = Process::real().output(&request).expect("run current test executable");
        assert!(output.success);
        assert!(!output.stdout.is_empty());
    }
}
